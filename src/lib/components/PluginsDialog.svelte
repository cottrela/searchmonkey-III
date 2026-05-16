<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';
  import { filename } from '$lib/paths';
  import type {
    InstalledPluginInfo,
    PluginHealthSummary,
    PluginIndexStatus,
    PluginIssue
  } from '$lib/types';

  type PluginDialogPage = 'installed' | 'available' | 'updates' | 'install';
  type IssueCategory = {
    code: string;
    label: string;
    count: number;
    pluginId: string;
    autoIgnored: boolean;
  };
  const KNOWN_ISSUE_TYPES = [
    { code: 'cloud_file_unavailable', label: 'Cloud file unavailable' },
    { code: 'pdf_open_failed', label: 'Could not open PDF' },
    { code: 'encrypted_pdf', label: 'Encrypted PDF' },
    { code: 'corrupt_pdf', label: 'Corrupt PDF' },
    { code: 'plugin_timeout', label: 'Plugin timed out' },
    { code: 'missing_source', label: 'Source file missing' },
    { code: 'skipped', label: 'Skipped' }
  ] as const;

  let {
    status,
    selectedPluginId = null,
    initialPage = 'installed',
    onClose,
    onRefresh,
    onOpenFolder,
    onRebuild,
    onOpenPluginFolder,
    onRefreshPlugin,
    onResetPlugin,
    onSetPluginEnabled,
    onInstallPlugin,
    onRetryFailure,
    onRevealFailure,
    onIgnoreFailure,
    onUnignoreFailure,
    onRetryIssueType,
    onIgnoreIssueType,
    onAutoIgnoreIssueType,
    onActivateVersion,
    onUninstallVersion
  }: {
    status: PluginIndexStatus | null;
    selectedPluginId?: string | null;
    initialPage?: PluginDialogPage;
    onClose?: () => void;
    onRefresh?: () => void;
    onOpenFolder?: () => void;
    onRebuild?: () => void;
    onOpenPluginFolder?: (path: string) => void | Promise<void>;
    onRefreshPlugin?: (pluginId: string) => void | Promise<void>;
    onResetPlugin?: (pluginId: string) => void | Promise<void>;
    onSetPluginEnabled?: (pluginId: string, enabled: boolean) => void | Promise<void>;
    onInstallPlugin?: (archivePath: string) => void | Promise<void>;
    onRetryFailure?: (path: string) => void | Promise<void>;
    onRevealFailure?: (path: string) => void | Promise<void>;
    onIgnoreFailure?: (path: string, pluginId: string) => void | Promise<void>;
    onUnignoreFailure?: (path: string, pluginId: string) => void | Promise<void>;
    onRetryIssueType?: (pluginId: string, errorCode: string) => void | Promise<void>;
    onIgnoreIssueType?: (pluginId: string, errorCode: string) => void | Promise<void>;
    onAutoIgnoreIssueType?: (pluginId: string, errorCode: string, enabled: boolean) => void | Promise<void>;
    onActivateVersion?: (pluginId: string, version: string) => void | Promise<void>;
    onUninstallVersion?: (pluginId: string, version: string) => void | Promise<void>;
  } = $props();

  let currentPage = $state<PluginDialogPage>('installed');
  let internalSelectedPluginId = $state<string | null>(null);
  let selectedIssueCode = $state<string | null>(null);
  let openIssueDetails = $state<Record<string, boolean>>({});
  let showIgnoredIssues = $state(false);
  let pluginsDialogElement = $state<HTMLElement>();
  let pendingRetryPaths = $state<Record<string, boolean>>({});
  let pendingRevealPaths = $state<Record<string, boolean>>({});
  let pendingUnignorePaths = $state<Record<string, boolean>>({});
  let pendingVersionActivations = $state<Record<string, boolean>>({});
  let pendingVersionUninstalls = $state<Record<string, boolean>>({});
  let pendingPluginToggles = $state<Record<string, boolean>>({});
  let pendingIssueTypeActions = $state<Record<string, boolean>>({});
  let hiddenIgnoredPaths = $state<Record<string, boolean>>({});
  let hiddenRetriedPaths = $state<Record<string, string>>({});
  let installStatus = $state<'ready' | 'installing' | 'success' | 'failed'>('ready');
  let installMessage = $state('');
  let installDropActive = $state(false);

  $effect(() => {
    currentPage = initialPage;
  });

  $effect(() => {
    if (selectedPluginId) {
      internalSelectedPluginId = selectedPluginId;
      currentPage = 'installed';
    }
  });

  $effect(() => {
    if (!status) return;

    const nextHiddenRetriedPaths: Record<string, string> = {};
    for (const issue of status.issues) {
      const hiddenTimestamp = hiddenRetriedPaths[issue.source_path];
      if (!hiddenTimestamp) continue;
      if (issue.last_reported_at === hiddenTimestamp) nextHiddenRetriedPaths[issue.source_path] = hiddenTimestamp;
    }
    const currentKeys = Object.keys(hiddenRetriedPaths);
    const nextKeys = Object.keys(nextHiddenRetriedPaths);
    if (
      currentKeys.length === nextKeys.length &&
      currentKeys.every((key) => nextHiddenRetriedPaths[key] === hiddenRetriedPaths[key])
    ) {
      return;
    }
    hiddenRetriedPaths = nextHiddenRetriedPaths;
  });

  const installedPlugins = $derived(status?.installed_plugins ?? []);
  const pluginGroups = $derived.by(() => {
    const groups = new Map<string, InstalledPluginInfo>();
    for (const plugin of installedPlugins) {
      const existing = groups.get(plugin.id);
      if (!existing || plugin.is_active) groups.set(plugin.id, plugin);
    }
    return [...groups.values()];
  });
  const selectedPluginIdValue = $derived.by(() => {
    if (!pluginGroups.length) return null;
    if (internalSelectedPluginId) {
      return pluginGroups.find((plugin) => plugin.id === internalSelectedPluginId)?.id ?? pluginGroups[0].id;
    }
    return pluginGroups[0].id;
  });
  const selectedPlugin = $derived.by(() => {
    if (!selectedPluginIdValue) return null;
    return installedPlugins.find((plugin) => plugin.id === selectedPluginIdValue && plugin.is_active)
      ?? installedPlugins.find((plugin) => plugin.id === selectedPluginIdValue)
      ?? null;
  });
  const selectedPluginVersions = $derived.by(() => {
    if (!selectedPluginIdValue) return [];
    return installedPlugins
      .filter((plugin) => plugin.id === selectedPluginIdValue)
      .sort((left, right) => right.version.localeCompare(left.version, undefined, { numeric: true }));
  });
  const selectedSummary = $derived.by<PluginHealthSummary | null>(() => {
    if (!status || !selectedPluginIdValue) return null;
    return status.plugin_summaries.find((summary) => summary.plugin_id === selectedPluginIdValue) ?? null;
  });
  const autoIgnoredIssueCodes = $derived.by<Set<string>>(() => {
    if (!status || !selectedPluginIdValue) return new Set();
    return new Set(
      status.auto_ignored_issue_types
        .filter((item) => item.plugin_id === selectedPluginIdValue)
        .map((item) => item.error_code)
    );
  });
  const pluginIssues = $derived.by<PluginIssue[]>(() => {
    if (!status || !selectedPluginIdValue) return [];
    let issues = status.issues.filter((issue) => issue.plugin_id === selectedPluginIdValue);
    issues = issues.filter((issue) => !hiddenIgnoredPaths[issue.source_path]);
    issues = issues.filter((issue) => hiddenRetriedPaths[issue.source_path] !== issue.last_reported_at);
    return issues;
  });
  const visibleIssues = $derived.by<PluginIssue[]>(() => {
    let issues = pluginIssues;
    if (!showIgnoredIssues) issues = issues.filter((issue) => issue.status !== 'ignored');
    return issues;
  });
  const activeIssues = $derived.by<PluginIssue[]>(() => visibleIssues.filter((issue) => issue.status !== 'ignored'));
  const activeIssueCount = $derived(activeIssues.length);
  const selectedIssues = $derived.by<PluginIssue[]>(() => {
    let issues = visibleIssues;
    if (selectedIssueCode) issues = issues.filter((issue) => issue.error_code === selectedIssueCode);
    return issues;
  });
  const issueCategories = $derived.by<IssueCategory[]>(() => {
    if (!selectedPluginIdValue) return [];
    const counts = new Map<string, IssueCategory>();
    for (const issueType of KNOWN_ISSUE_TYPES) {
      counts.set(issueType.code, {
        code: issueType.code,
        label: issueType.label,
        count: 0,
        pluginId: selectedPluginIdValue,
        autoIgnored: autoIgnoredIssueCodes.has(issueType.code)
      });
    }
    for (const issueCode of autoIgnoredIssueCodes) {
      if (counts.has(issueCode)) continue;
      counts.set(issueCode, {
        code: issueCode,
        label: labelForIssueCode(issueCode),
        count: 0,
        pluginId: selectedPluginIdValue,
        autoIgnored: true
      });
    }
    for (const issue of activeIssues) {
      const existing = counts.get(issue.error_code);
      if (existing) {
        existing.count += 1;
        existing.autoIgnored = autoIgnoredIssueCodes.has(issue.error_code);
        continue;
      }
      counts.set(issue.error_code, {
        code: issue.error_code,
        label: labelForIssueCode(issue.error_code, issue.message),
        count: 1,
        pluginId: selectedPluginIdValue,
        autoIgnored: autoIgnoredIssueCodes.has(issue.error_code)
      });
    }
    return [...counts.values()].sort(
      (left, right) =>
        left.label.localeCompare(right.label, undefined, { sensitivity: 'base' })
        || left.code.localeCompare(right.code, undefined, { sensitivity: 'base' })
    );
  });
  const ignoredIssueCount = $derived.by(() => {
    return selectedSummary?.ignored_count ?? 0;
  });
  const indexingLabel = $derived.by(() => {
    if (!status) return 'Idle';
    if (selectedPlugin && !selectedPlugin.enabled) return 'Plugin is disabled';
    if (status.paused) return 'Processing paused';
    if (status.search_active && (selectedSummary?.queued_count ?? 0) > 0) {
      return `Waiting for search to finish (${selectedSummary?.queued_count ?? 0} queued)`;
    }
    if (status.plugin_state === 'working') {
      const queued = selectedSummary?.queued_count ?? 0;
      const processing = selectedSummary?.processing_count ?? 0;
      if (queued > 0) return `${queued} queued`;
      if (processing > 0) return 'Working';
      return 'Working';
    }
    return 'Idle';
  });

  $effect(() => {
    if (!selectedIssueCode) return;
    if (issueCategories.some((category) => category.code === selectedIssueCode)) return;
    selectedIssueCode = null;
  });

  function selectPlugin(plugin: InstalledPluginInfo) {
    internalSelectedPluginId = plugin.id;
    selectedIssueCode = null;
    currentPage = 'installed';
  }

  function labelForIssue(issue: PluginIssue): string {
    return labelForIssueCode(issue.error_code, issue.message);
  }

  function labelForIssueCode(errorCode: string, fallbackMessage?: string): string {
    switch (errorCode) {
      case 'cloud_file_unavailable':
        return 'Cloud file unavailable';
      case 'pdf_open_failed':
        return 'Could not open PDF';
      case 'encrypted_pdf':
        return 'Encrypted PDF';
      case 'corrupt_pdf':
        return 'Corrupt PDF';
      case 'plugin_timeout':
        return 'Plugin timed out';
      case 'stale_source':
        return 'Needs reprocessing';
      case 'missing_source':
        return 'Source file missing';
      default:
        return fallbackMessage ?? errorCode;
    }
  }

  function retryMessage(retryAfter?: string | null): string | null {
    if (!retryAfter) return null;
    const retryTime = new Date(retryAfter).getTime();
    if (!Number.isFinite(retryTime)) return 'Automatic retry later';
    const deltaMs = retryTime - Date.now();
    if (deltaMs <= 0) return 'Automatic retry due';
    const minutes = Math.ceil(deltaMs / 60000);
    if (minutes < 60) return `Automatic retry in ${minutes} minute${minutes === 1 ? '' : 's'}`;
    const hours = Math.ceil(minutes / 60);
    if (hours < 24) return `Automatic retry in ${hours} hour${hours === 1 ? '' : 's'}`;
    const days = Math.ceil(hours / 24);
    return `Automatic retry in ${days} day${days === 1 ? '' : 's'}`;
  }

  function detailKey(issue: PluginIssue) {
    return `${issue.plugin_id}:${issue.source_path}:${issue.error_code}`;
  }

  function isIssueExpanded(issue: PluginIssue) {
    return openIssueDetails[detailKey(issue)] ?? false;
  }

  function setIssueExpanded(issue: PluginIssue, expanded: boolean) {
    openIssueDetails = { ...openIssueDetails, [detailKey(issue)]: expanded };
  }

  function closePluginMenus(except?: HTMLDetailsElement) {
    pluginsDialogElement?.querySelectorAll<HTMLDetailsElement>('.menu[open]').forEach((menu) => {
      if (menu !== except) menu.open = false;
    });
  }

  function handlePluginMenuToggle(event: Event) {
    const menu = event.currentTarget;
    if (!(menu instanceof HTMLDetailsElement) || !menu.open) return;
    closePluginMenus(menu);
  }

  function handlePluginMenuFocusOut(event: FocusEvent) {
    const menu = event.currentTarget;
    if (!(menu instanceof HTMLDetailsElement)) return;

    setTimeout(() => {
      if (menu.contains(document.activeElement)) return;
      menu.open = false;
    }, 120);
  }

  function truncateMiddle(value: string, maxLength = 56) {
    if (value.length <= maxLength) return value;
    if (maxLength <= 3) return value.slice(0, maxLength);
    const visibleChars = maxLength - 1;
    const head = Math.ceil(visibleChars / 2);
    const tail = Math.floor(visibleChars / 2);
    return `${value.slice(0, head)}…${value.slice(-tail)}`;
  }

  function truncateFilenameMiddle(filePath: string, maxLength = 84) {
    const name = filename(filePath);
    const dotIndex = name.lastIndexOf('.');
    if (name.length <= maxLength) return name;
    if (dotIndex <= 0 || dotIndex === name.length - 1) return truncateMiddle(name, maxLength);
    return `${truncateMiddle(name.slice(0, dotIndex), maxLength - name.slice(dotIndex).length)}${name.slice(dotIndex)}`;
  }

  function markPending(record: Record<string, boolean>, path: string, pending: boolean) {
    return { ...record, [path]: pending };
  }

  function markHiddenRetry(record: Record<string, string>, path: string, timestamp: string) {
    return { ...record, [path]: timestamp };
  }

  function issueTypeActionKey(pluginId: string, errorCode: string, action: string) {
    return `${pluginId}:${errorCode}:${action}`;
  }

  function isIssueTypeActionPending(pluginId: string, errorCode: string, action: string) {
    return pendingIssueTypeActions[issueTypeActionKey(pluginId, errorCode, action)] ?? false;
  }

  function toggleIssueCategory(category: IssueCategory) {
    selectedIssueCode = selectedIssueCode === category.code ? null : category.code;
  }

  async function queueRetry(path: string, lastReportedAt: string) {
    if (!onRetryFailure || pendingRetryPaths[path]) return;
    pendingRetryPaths = markPending(pendingRetryPaths, path, true);
    hiddenRetriedPaths = markHiddenRetry(hiddenRetriedPaths, path, lastReportedAt);
    try {
      await onRetryFailure(path);
    } finally {
      pendingRetryPaths = markPending(pendingRetryPaths, path, false);
    }
  }

  async function revealIssue(path: string) {
    if (!onRevealFailure || pendingRevealPaths[path]) return;
    pendingRevealPaths = markPending(pendingRevealPaths, path, true);
    try {
      await onRevealFailure(path);
    } finally {
      pendingRevealPaths = markPending(pendingRevealPaths, path, false);
    }
  }

  async function ignoreIssue(path: string, pluginId: string) {
    if (!onIgnoreFailure) return;
    hiddenIgnoredPaths = markPending(hiddenIgnoredPaths, path, true);
    try {
      await onIgnoreFailure(path, pluginId);
      hiddenIgnoredPaths = markPending(hiddenIgnoredPaths, path, false);
    } catch (error) {
      hiddenIgnoredPaths = markPending(hiddenIgnoredPaths, path, false);
      throw error;
    }
  }

  async function unignoreIssue(path: string, pluginId: string) {
    if (!onUnignoreFailure || pendingUnignorePaths[path]) return;
    pendingUnignorePaths = markPending(pendingUnignorePaths, path, true);
    try {
      await onUnignoreFailure(path, pluginId);
    } finally {
      pendingUnignorePaths = markPending(pendingUnignorePaths, path, false);
    }
  }

  async function retryIssueType(category: IssueCategory) {
    if (!onRetryIssueType) return;
    const key = issueTypeActionKey(category.pluginId, category.code, 'retry');
    if (pendingIssueTypeActions[key]) return;
    pendingIssueTypeActions = markPending(pendingIssueTypeActions, key, true);
    try {
      await onRetryIssueType(category.pluginId, category.code);
    } finally {
      pendingIssueTypeActions = markPending(pendingIssueTypeActions, key, false);
    }
  }

  async function ignoreIssueType(category: IssueCategory) {
    if (!onIgnoreIssueType) return;
    const key = issueTypeActionKey(category.pluginId, category.code, 'ignore');
    if (pendingIssueTypeActions[key]) return;
    pendingIssueTypeActions = markPending(pendingIssueTypeActions, key, true);
    try {
      await onIgnoreIssueType(category.pluginId, category.code);
    } finally {
      pendingIssueTypeActions = markPending(pendingIssueTypeActions, key, false);
    }
  }

  async function autoIgnoreIssueType(category: IssueCategory, enabled: boolean) {
    if (!onAutoIgnoreIssueType) return;
    const key = issueTypeActionKey(category.pluginId, category.code, 'auto-ignore');
    if (pendingIssueTypeActions[key]) return;
    pendingIssueTypeActions = markPending(pendingIssueTypeActions, key, true);
    try {
      await onAutoIgnoreIssueType(category.pluginId, category.code, enabled);
    } finally {
      pendingIssueTypeActions = markPending(pendingIssueTypeActions, key, false);
    }
  }

  function versionKey(pluginId: string, version: string) {
    return `${pluginId}@${version}`;
  }

  async function activateVersion(pluginId: string, version: string) {
    if (!onActivateVersion) return;
    const key = versionKey(pluginId, version);
    if (pendingVersionActivations[key]) return;
    pendingVersionActivations = markPending(pendingVersionActivations, key, true);
    try {
      await onActivateVersion(pluginId, version);
    } finally {
      pendingVersionActivations = markPending(pendingVersionActivations, key, false);
    }
  }

  async function uninstallVersion(pluginId: string, version: string) {
    if (!onUninstallVersion) return;
    if (!window.confirm(`Uninstall plugin version ${version}?`)) return;
    const key = versionKey(pluginId, version);
    if (pendingVersionUninstalls[key]) return;
    pendingVersionUninstalls = markPending(pendingVersionUninstalls, key, true);
    try {
      await onUninstallVersion(pluginId, version);
    } finally {
      pendingVersionUninstalls = markPending(pendingVersionUninstalls, key, false);
    }
  }

  function issueStatusText(issue: PluginIssue) {
    return retryMessage(issue.retry_after);
  }

  async function browseForPluginPackage() {
    const selected = await open({
      multiple: false,
      filters: [{ name: 'Searchmonkey Plugin', extensions: ['smplugin'] }]
    });
    if (typeof selected !== 'string') return;
    await installArchive(selected);
  }

  async function installArchive(archivePath: string) {
    if (!onInstallPlugin) return;
    installStatus = 'installing';
    installMessage = 'Installing';
    try {
      await onInstallPlugin(archivePath);
      installStatus = 'success';
      installMessage = 'Success';
    } catch (error) {
      installStatus = 'failed';
      installMessage = error instanceof Error ? error.message : 'Install failed';
    }
  }

  function setPage(page: PluginDialogPage) {
    currentPage = page;
  }

  async function refreshSelectedPlugin() {
    if (!selectedPlugin || !onRefreshPlugin) return;
    await onRefreshPlugin(selectedPlugin.id);
  }

  async function resetSelectedPlugin() {
    if (!selectedPlugin || !onResetPlugin) return;
    if (!window.confirm(`Reset cached output for ${selectedPlugin.name}?`)) return;
    await onResetPlugin(selectedPlugin.id);
  }

  async function toggleSelectedPluginEnabled() {
    if (!selectedPlugin || !onSetPluginEnabled) return;
    const nextEnabled = !selectedPlugin.enabled;
    if (!nextEnabled) {
      const confirmed = window.confirm(
        `Disable ${selectedPlugin.name}? This also clears its cached output so it stops affecting search results.`
      );
      if (!confirmed) return;
    }

    pendingPluginToggles = { ...pendingPluginToggles, [selectedPlugin.id]: true };
    try {
      await onSetPluginEnabled(selectedPlugin.id, nextEnabled);
    } finally {
      pendingPluginToggles = { ...pendingPluginToggles, [selectedPlugin.id]: false };
    }
  }

  async function uninstallSelectedPlugin() {
    if (!selectedPlugin) return;
    await uninstallVersion(selectedPlugin.id, selectedPlugin.version);
  }

  function handleDropHover(event: DragEvent) {
    event.preventDefault();
    installDropActive = true;
  }

  function handleDropLeave(event: DragEvent) {
    event.preventDefault();
    installDropActive = false;
  }

  async function handleDrop(event: DragEvent) {
    event.preventDefault();
    installDropActive = false;
    const dropped = event.dataTransfer?.files?.[0] as (File & { path?: string }) | undefined;
    const filePath = dropped?.path;
    if (filePath) {
      await installArchive(filePath);
      return;
    }
    installStatus = 'failed';
    installMessage = 'Use Browse to pick a local .smplugin file';
  }

  $effect(() => {
    const handlePointerDown = (event: PointerEvent) => {
      if (!pluginsDialogElement) return;
      if (!(event.target instanceof Node)) return;
      const menu = (event.target instanceof Element ? event.target : event.target.parentElement)?.closest('.menu');
      if (menu && pluginsDialogElement.contains(menu)) return;
      closePluginMenus();
    };

    document.addEventListener('pointerdown', handlePointerDown, true);
    return () => {
      document.removeEventListener('pointerdown', handlePointerDown, true);
    };
  });
</script>

<div class="modal-layer" role="presentation">
  <button class="modal-backdrop" type="button" aria-label="Close plugin manager" onclick={onClose}></button>

  <div bind:this={pluginsDialogElement} class="plugins-dialog" role="dialog" aria-modal="true" aria-labelledby="plugins-title">
    <aside class="sidebar">
      <div class="sidebar-header">
        <h2 id="plugins-title">Plugins</h2>
        <button class="close-dialog" type="button" aria-label="Close plugin manager" onclick={onClose}>×</button>
      </div>

      <nav class="nav-groups" aria-label="Plugin pages">
        <button type="button" class:active={currentPage === 'installed'} onclick={() => setPage('installed')}>Installed</button>
        <button type="button" class:active={currentPage === 'available'} onclick={() => setPage('available')}>Available</button>
        <button type="button" class:active={currentPage === 'updates'} onclick={() => setPage('updates')}>Updates</button>
        <button type="button" class:active={currentPage === 'install'} onclick={() => setPage('install')}>Install New Plugin</button>
      </nav>

      {#if currentPage === 'installed' && pluginGroups.length}
        <div class="plugin-list">
          {#each pluginGroups as plugin}
            <button type="button" class:selected={selectedPluginIdValue === plugin.id} onclick={() => selectPlugin(plugin)}>
              <span>{plugin.name}</span>
              <span>v{plugin.version}</span>
            </button>
          {/each}
        </div>
      {/if}
    </aside>

    <section class="detail plugin-panel">
      {#if currentPage === 'install'}
        <div class="plugin-content">
        <header class="detail-header">
          <div>
            <h3>Install New Plugin</h3>
            <p class="muted">Install a local `.smplugin` package.</p>
          </div>
        </header>

        <section class="panel install-panel">
          <button
            type="button"
            class:drag-active={installDropActive}
            class="drop-zone"
            ondragenter={handleDropHover}
            ondragover={handleDropHover}
            ondragleave={handleDropLeave}
            ondrop={handleDrop}
            onclick={browseForPluginPackage}
          >
            <strong>Drop `.smplugin` here</strong>
            <span>or Browse…</span>
          </button>

          <div class="install-status">
            <span class="detail-label">Status</span>
            <strong>{installStatus === 'ready' ? 'Ready' : installStatus === 'installing' ? 'Installing' : installStatus === 'success' ? 'Success' : 'Failed'}</strong>
          </div>
          {#if installMessage}
            <p class="muted">{installMessage}</p>
          {/if}
        </section>
        </div>
      {:else if currentPage === 'available'}
        <div class="empty-state plugin-content">
          <h3>Available</h3>
          <p>Remote plugin catalog wiring is not in place yet.</p>
          <button type="button" class="secondary" onclick={() => setPage('install')}>Install New Plugin</button>
        </div>
      {:else if currentPage === 'updates'}
        <div class="empty-state plugin-content">
          <h3>Updates</h3>
          <p>Installed version switching exists. Update discovery is not wired yet.</p>
        </div>
      {:else if selectedPlugin}
        <div class="plugin-content">
        <header class="detail-header">
          <div>
            <h3>{selectedPlugin.name}</h3>
            <p>v{selectedPlugin.version}</p>
          </div>

          <details class="menu" onfocusout={handlePluginMenuFocusOut} ontoggle={handlePluginMenuToggle}>
            <summary>More…</summary>
            <div class="menu-panel compact">
              <button
                type="button"
                disabled={pendingPluginToggles[selectedPlugin.id]}
                onclick={toggleSelectedPluginEnabled}
              >
                {#if pendingPluginToggles[selectedPlugin.id]}
                  {selectedPlugin.enabled ? 'Disabling…' : 'Enabling…'}
                {:else}
                  {selectedPlugin.enabled ? 'Disable Plugin' : 'Enable Plugin'}
                {/if}
              </button>
              <button type="button" onclick={() => onOpenPluginFolder?.(selectedPlugin.root_path)}>Open Plugin Folder</button>
              <button type="button" onclick={() => (showIgnoredIssues = !showIgnoredIssues)}>
                {showIgnoredIssues ? 'Hide Ignored Files' : 'Show Ignored Files'}
              </button>
              <button type="button" onclick={refreshSelectedPlugin}>Refresh Supported Files</button>
              <div class="menu-separator" aria-hidden="true"></div>
              <button type="button" onclick={uninstallSelectedPlugin}>Uninstall…</button>
              <button type="button" onclick={resetSelectedPlugin}>Reset This Plugin Cache…</button>
            </div>
          </details>
        </header>

        <p class="description">Extracts searchable text from {selectedPlugin.handles.join(', ')} files.</p>

        <section class="panel">
          <h4>Status</h4>
          <p class="summary-line">
            <strong>{selectedSummary?.indexed_count ?? 0} processed</strong>
            <span>·</span>
            <strong>{activeIssueCount} need attention</strong>
          </p>
          <div class="status-line">
            <p class="muted">{indexingLabel}</p>
            {#if selectedPlugin && !selectedPlugin.enabled}
              <button
                type="button"
                class="secondary status-action"
                disabled={pendingPluginToggles[selectedPlugin.id]}
                onclick={toggleSelectedPluginEnabled}
              >
                {pendingPluginToggles[selectedPlugin.id] ? 'Enabling…' : 'Re-enable'}
              </button>
            {/if}
          </div>
        </section>

        <section class="panel">
          <h4>Capabilities</h4>
          <p class="chips">
            <span>Text extraction</span>
            <span>{selectedPlugin.capabilities.layout ? 'Layout preservation' : 'Plain text only'}</span>
            <span>{selectedPlugin.capabilities.ocr ? 'OCR' : 'No OCR'}</span>
          </p>
        </section>

        <section class="panel">
          <h4>Storage</h4>
          <button type="button" class="linkish" onclick={() => onOpenPluginFolder?.(selectedPlugin.root_path)}>Open plugin folder</button>
        </section>

        <section class="panel">
          <h4>Versions</h4>
          <div class="plugin-versions">
            {#each selectedPluginVersions as pluginVersion}
              <div class="version-row">
                <div class="version-label">
                  <strong>v{pluginVersion.version}</strong>
                  {#if pluginVersion.is_active}
                    <span class="active-pill">Active</span>
                  {/if}
                </div>
                <div class="version-actions">
                  {#if !pluginVersion.is_active}
                    <button
                      type="button"
                      class="secondary"
                      disabled={pendingVersionActivations[versionKey(pluginVersion.id, pluginVersion.version)]}
                      onclick={() => activateVersion(pluginVersion.id, pluginVersion.version)}
                    >
                      {pendingVersionActivations[versionKey(pluginVersion.id, pluginVersion.version)] ? 'Switching…' : 'Set active'}
                    </button>
                  {/if}
                  <button
                    type="button"
                    class="secondary"
                    disabled={pendingVersionUninstalls[versionKey(pluginVersion.id, pluginVersion.version)]}
                    onclick={() => uninstallVersion(pluginVersion.id, pluginVersion.version)}
                  >
                    {pendingVersionUninstalls[versionKey(pluginVersion.id, pluginVersion.version)] ? 'Uninstalling…' : 'Uninstall'}
                  </button>
                </div>
              </div>
            {/each}
          </div>
        </section>

        <section class="panel issues-panel">
          <div class="panel-header">
            <div>
              <h4>Issues</h4>
              <p class="muted">
                {activeIssueCount} files need attention{#if ignoredIssueCount} · {ignoredIssueCount} ignored{/if}
              </p>
            </div>
          </div>

          {#if issueCategories.length}
            <div class="issue-categories">
              {#each issueCategories as category}
                <div class="issue-category-group">
                  <button
                    type="button"
                    class:selected={selectedIssueCode === category.code}
                    class="issue-category-pill"
                    onclick={() => toggleIssueCategory(category)}
                  >
                    <span>{category.label}</span>
                    <strong>{category.count}</strong>
                    <span class="chevron">{selectedIssueCode === category.code ? '▴' : '▾'}</span>
                  </button>
                  {#if selectedIssueCode === category.code}
                    <div class="issue-category-actions">
                      <button
                        type="button"
                        class="secondary"
                        disabled={isIssueTypeActionPending(category.pluginId, category.code, 'retry')}
                        onclick={() => retryIssueType(category)}
                      >
                        {isIssueTypeActionPending(category.pluginId, category.code, 'retry') ? 'Queueing…' : 'Retry all'}
                      </button>
                      <button
                        type="button"
                        class="secondary"
                        disabled={isIssueTypeActionPending(category.pluginId, category.code, 'ignore')}
                        onclick={() => ignoreIssueType(category)}
                      >
                        {isIssueTypeActionPending(category.pluginId, category.code, 'ignore') ? 'Ignoring…' : 'Ignore all'}
                      </button>
                      <button
                        type="button"
                        class="secondary auto-ignore"
                        disabled={isIssueTypeActionPending(category.pluginId, category.code, 'auto-ignore')}
                        onclick={() => autoIgnoreIssueType(category, !category.autoIgnored)}
                      >
                        {#if isIssueTypeActionPending(category.pluginId, category.code, 'auto-ignore')}
                          Saving…
                        {:else if category.autoIgnored}
                          Disable auto-ignore
                        {:else}
                          Always ignore this issue type
                        {/if}
                      </button>
                    </div>
                  {/if}
                </div>
              {/each}
            </div>

            <div class="issues-list">
              {#each selectedIssues as issue}
                <article class:ignored-card={issue.status === 'ignored'} class="issue-card">
                  <div class="issue-copy">
                    <strong title={issue.file_name}>{truncateFilenameMiddle(issue.file_name)}</strong>
                    <p>
                      {labelForIssue(issue)}
                      {#if issue.status === 'ignored'}
                        <span class="ignored-badge">Ignored</span>
                      {/if}
                    </p>
                    {#if issueStatusText(issue)}
                      <p class="muted">{issueStatusText(issue)}</p>
                    {/if}
                  </div>
                  <div class="issue-actions">
                    {#if issue.status !== 'queued' && issue.status !== 'processing'}
                      <button
                        type="button"
                        class="secondary"
                        disabled={pendingRetryPaths[issue.source_path]}
                        onclick={() => queueRetry(issue.source_path, issue.last_reported_at)}
                      >
                        {pendingRetryPaths[issue.source_path] ? 'Queued' : 'Retry now'}
                      </button>
                    {/if}
                    <button
                      type="button"
                      class="secondary"
                      disabled={pendingRevealPaths[issue.source_path]}
                      onclick={() => revealIssue(issue.source_path)}
                    >
                      {pendingRevealPaths[issue.source_path] ? 'Revealing…' : 'Reveal'}
                    </button>
                    {#if issue.status === 'ignored'}
                      <button
                        type="button"
                        class="secondary"
                        disabled={pendingUnignorePaths[issue.source_path]}
                        onclick={() => unignoreIssue(issue.source_path, issue.plugin_id)}
                      >
                        {pendingUnignorePaths[issue.source_path] ? 'Re-enabling…' : 'Re-enable issue'}
                      </button>
                    {:else}
                      <button type="button" class="secondary" onclick={() => ignoreIssue(issue.source_path, issue.plugin_id)}>
                        Ignore
                      </button>
                    {/if}
                    <details
                      class="details"
                      open={isIssueExpanded(issue)}
                      ontoggle={(event) => setIssueExpanded(issue, (event.currentTarget as HTMLDetailsElement).open)}
                    >
                      <summary>{isIssueExpanded(issue) ? 'Hide details ▴' : 'Details ▾'}</summary>
                      <div class="details-copy">
                        <div class="detail-row">
                          <span class="detail-label">Full path</span>
                          <code>{issue.source_path}</code>
                        </div>
                        <div class="detail-row">
                          <span class="detail-label">Error code</span>
                          <code>{issue.error_code}</code>
                        </div>
                        <div class="detail-row">
                          <span class="detail-label">Attempts</span>
                          <code>{issue.attempts}</code>
                        </div>
                        <div class="detail-row raw-output">
                          <span class="detail-label">Raw plugin output</span>
                          <pre>{issue.details}</pre>
                        </div>
                      </div>
                    </details>
                  </div>
                </article>
              {/each}
            </div>
          {:else}
            <p class="muted">No issues.</p>
          {/if}
        </section>
        </div>
      {:else}
        <div class="empty-state plugin-content">
          <h3>No Plugins Installed</h3>
          <p>Open the plugins folder or install a local package.</p>
          <div class="empty-actions">
            <button type="button" class="secondary" onclick={onOpenFolder}>Open Plugins Folder</button>
            <button type="button" class="secondary" onclick={() => setPage('install')}>Install New Plugin</button>
          </div>
        </div>
      {/if}

    </section>
  </div>
</div>

<style>
  .modal-layer {
    position: fixed;
    inset: 0;
    z-index: 48;
    display: grid;
    place-items: center;
    padding: 24px;
  }

  .modal-backdrop {
    position: absolute;
    inset: 0;
    border: 0;
    background: rgba(30, 37, 45, 0.2);
  }

  .plugins-dialog {
    position: relative;
    z-index: 1;
    display: grid;
    grid-template-columns: 250px minmax(560px, 1fr);
    width: min(1020px, calc(100vw - 36px));
    height: min(720px, calc(100vh - 48px));
    max-height: calc(100vh - 48px);
    border: 1px solid #d9e0d9;
    border-radius: 14px;
    background: #ffffff;
    box-shadow: 0 22px 44px rgba(27, 35, 42, 0.16);
    overflow: hidden;
  }

  .sidebar {
    display: grid;
    grid-auto-rows: min-content;
    gap: 14px;
    min-height: 0;
    padding: 20px 14px;
    border-right: 1px solid #e2e7e2;
    background: #f6f8f6;
    overflow: auto;
  }

  .sidebar h2,
  .detail-header h3,
  .panel h4 {
    margin: 0;
  }

  .sidebar-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 0 10px;
  }

  .sidebar h2 {
    color: #1c232b;
    font-size: 18px;
    font-weight: 760;
  }

  .close-dialog {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    border: 1px solid #d8dfd8;
    border-radius: 999px;
    color: #4d5965;
    background: #fff;
    cursor: pointer;
    font-size: 18px;
    line-height: 1;
  }

  .nav-groups,
  .plugin-list,
  .issue-categories,
  .menu-panel {
    display: grid;
    gap: 6px;
  }

  .nav-groups button,
  .plugin-list button,
  .issue-category-pill,
  .issue-category-actions button,
  .menu-panel button,
  .drop-zone {
    font: inherit;
  }

  .nav-groups button,
  .plugin-list button {
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 38px;
    border: 0;
    border-radius: 8px;
    padding: 0 10px;
    color: #2e3842;
    background: transparent;
    cursor: pointer;
    text-align: left;
  }

  .nav-groups button:hover,
  .plugin-list button:hover {
    background: #eef3ee;
  }

  .nav-groups button.active,
  .plugin-list button.selected {
    background: #e7efe7;
    color: #0f6b3b;
    font-weight: 700;
  }

  .plugin-list span:last-child,
  .muted {
    color: #65707a;
    font-size: 13px;
  }

  .detail {
    display: grid;
    min-height: 0;
    overflow: auto;
  }

  .plugin-panel {
    align-items: stretch;
  }

  .plugin-content {
    display: grid;
    gap: 18px;
    padding: 28px 36px;
    justify-content: flex-start;
    align-items: stretch;
    align-content: start;
    min-height: 100%;
  }

  .detail-header,
  .panel-header {
    display: flex;
    align-items: start;
    justify-content: space-between;
    gap: 16px;
  }

  .description {
    margin: -8px 0 0;
    color: #49545e;
  }

  .panel {
    display: grid;
    gap: 10px;
    padding: 18px;
    border: 1px solid #e2e7e2;
    border-radius: 12px;
    background: #fbfcfb;
  }

  .summary-line,
  .chips,
  .empty-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin: 0;
  }

  .summary-line {
    gap: 10px;
    color: #1c232b;
    font-size: 18px;
  }

  .chips span,
  .active-pill,
  .ignored-badge {
    display: inline-flex;
    align-items: center;
    border-radius: 999px;
    font-size: 13px;
  }

  .chips span {
    border: 1px solid #d8dfd8;
    padding: 7px 11px;
    background: #fff;
    color: #31404d;
  }

  .active-pill {
    background: #e7efe7;
    color: #0f6b3b;
    padding: 2px 8px;
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .ignored-badge {
    margin-left: 8px;
    padding: 2px 7px;
    border: 1px solid #d6ddd6;
    background: #f4f6f4;
    color: #5d6873;
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.02em;
    vertical-align: middle;
  }

  .linkish,
  .secondary {
    border: 1px solid #d2dcd2;
    border-radius: 8px;
    padding: 8px 12px;
    color: #2c3740;
    background: #fff;
    cursor: pointer;
  }

  .linkish {
    width: fit-content;
    border: 0;
    padding: 0;
    background: transparent;
    color: #0f6b3b;
    text-decoration: underline;
    text-decoration-color: rgba(15, 107, 59, 0.35);
    text-underline-offset: 0.14em;
  }

  .plugin-versions,
  .issues-list {
    display: grid;
    gap: 12px;
  }

  .version-row,
  .issue-card,
  .install-status {
    display: flex;
    align-items: start;
    justify-content: space-between;
    gap: 12px;
  }

  .version-actions,
  .version-label,
  .issue-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
  }

  .issue-card {
    display: grid;
    gap: 10px;
    border: 1px solid #e2e7e2;
    border-radius: 12px;
    padding: 14px;
    background: #fff;
  }

  .ignored-card {
    opacity: 0.62;
    background: #fbfcfb;
  }

  .issue-copy p {
    margin: 0;
  }

  .issue-copy {
    display: grid;
    gap: 4px;
    min-width: 0;
  }

  .issue-copy strong {
    display: block;
    overflow: hidden;
    color: #1c232b;
    font-size: 14px;
    font-weight: 680;
    line-height: 1.35;
    white-space: nowrap;
    text-overflow: clip;
  }

  .details summary {
    cursor: pointer;
    width: fit-content;
    color: #45515d;
    font-size: 13px;
    user-select: none;
  }

  .details-copy {
    margin-top: 10px;
    display: grid;
    gap: 10px;
    color: #53606c;
    font-size: 12px;
    line-height: 1.5;
  }

  .detail-row {
    display: grid;
    gap: 4px;
  }

  .detail-label {
    color: #65707a;
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  code,
  pre {
    margin: 0;
    font-size: 12px;
  }

  code {
    overflow-wrap: anywhere;
    color: #2d3a46;
  }

  .raw-output pre {
    padding: 10px 12px;
    border: 1px solid #e8ece8;
    border-radius: 10px;
    background: #f7f9f7;
    color: #5b6773;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .status-line {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .status-line .muted {
    margin: 0;
  }

  .status-action {
    flex: 0 0 auto;
    min-height: 0;
    padding: 6px 10px;
    font-size: 13px;
  }

  .menu {
    position: relative;
  }

  .menu summary {
    list-style: none;
    cursor: pointer;
    border: 1px solid #d2dcd2;
    border-radius: 7px;
    padding: 5px 10px;
    background: #fff;
    color: #2c3740;
    font-size: 13px;
    line-height: 1.2;
  }

  .menu summary::-webkit-details-marker {
    display: none;
  }

  .menu-panel {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    min-width: 190px;
    border: 1px solid #d9e0d9;
    border-radius: 9px;
    padding: 5px;
    background: #fff;
    box-shadow: 0 16px 30px rgba(27, 35, 42, 0.12);
  }

  .menu-panel.compact button {
    justify-content: start;
    min-height: 0;
    border: 0;
    border-radius: 6px;
    padding: 6px 8px;
    background: transparent;
    text-align: left;
    font-size: 13px;
    line-height: 1.25;
  }

  .menu-panel.compact button:hover {
    background: #f3f6f3;
  }

  .menu-panel.compact button:disabled {
    color: #8a949d;
    cursor: not-allowed;
  }

  .menu-separator {
    height: 1px;
    margin: 4px 2px;
    background: #e2e7e2;
  }

  .issues-panel {
    gap: 14px;
  }

  .issue-categories {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .issue-category-group {
    display: grid;
    gap: 8px;
    align-content: start;
  }

  .issue-category-pill {
    display: inline-flex;
    flex: 0 0 auto;
    gap: 10px;
    align-items: center;
    border: 1px solid #d8dfd8;
    border-radius: 999px;
    padding: 8px 12px;
    background: #fff;
    color: #24313d;
    cursor: pointer;
    white-space: nowrap;
  }

  .issue-category-pill:hover {
    border-color: #bdd3c1;
    background: #f4faf6;
  }

  .issue-category-pill.selected {
    border-color: #0f6b3b;
    background: #edf7f1;
    color: #0f6b3b;
  }

  .issue-category-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    padding-left: 12px;
  }

  .issue-category-actions button {
    min-height: 0;
    padding: 6px 10px;
    font-size: 12px;
  }

  .issue-category-actions .auto-ignore {
    border-color: #bfd9c5;
  }

  .chevron {
    font-size: 12px;
    line-height: 1;
  }

  .issue-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .details {
    width: 100%;
    margin-top: 2px;
    padding-top: 10px;
    border-top: 1px solid #edf1ed;
  }

  .empty-state {
    display: grid;
    gap: 12px;
    min-height: 0;
    align-content: start;
    justify-content: flex-start;
    align-items: stretch;
  }

  .empty-state h3,
  .empty-state p {
    margin: 0;
  }

  .install-panel {
    gap: 16px;
  }

  .drop-zone {
    display: grid;
    align-items: center;
    justify-items: center;
    gap: 8px;
    min-height: 180px;
    border: 2px dashed #c7d4c7;
    border-radius: 14px;
    background: linear-gradient(180deg, #fbfcfb 0%, #f4f7f4 100%);
    color: #355049;
    cursor: pointer;
  }

  .drop-zone.drag-active {
    border-color: #0f6b3b;
    background: linear-gradient(180deg, #f3faf5 0%, #eaf5ee 100%);
  }

  @media (max-width: 900px) {
    .plugins-dialog {
      grid-template-columns: 1fr;
      height: min(760px, calc(100vh - 36px));
    }

    .sidebar {
      border-right: 0;
      border-bottom: 1px solid #e2e7e2;
    }

    .issue-card,
    .detail-header,
    .panel-header {
      display: grid;
    }

    .issue-actions {
      align-items: start;
    }
  }
</style>
