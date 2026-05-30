<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { filename } from '$lib/paths';
  import { getPluginIssueCounts, getPluginIssues } from '$lib/search';
  import type {
    InstalledPluginInfo,
    MarketplacePluginSummary,
    PluginHealthSummary,
    PluginIndexSummary,
    PluginIssue,
    PluginIssueCount,
    PurchaseConnectionSummary
  } from '$lib/types';

  const WEBSITE_URL = 'https://searchmonkey.dev';
  const PLUGINS_URL = `${WEBSITE_URL}/plugins`;

  type PluginDialogPage = 'installed' | 'available' | 'updates' | 'install';
  type MarketplaceBadgeTone = 'owned' | 'installed' | 'update' | 'development' | 'neutral';
  type IssueCategory = {
    code: string;
    label: string;
    count: number;
    pluginId: string;
    autoIgnored: boolean;
  };
  type PendingConfirmation = {
    title: string;
    message: string;
    confirmLabel: string;
    confirmTone?: 'danger' | 'default';
    resolve: (confirmed: boolean) => void;
  };
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
    onInstallMarketplacePlugin,
    onStartPurchaseVerification,
    onPollPendingPurchaseConnection,
    onRefreshPurchases,
    onDisconnectPurchases,
    onRetryFailure,
    onOpenFailure,
    onRevealFailure,
    onIgnoreFailure,
    onUnignoreFailure,
    onRetryIssueType,
    onIgnoreIssueType,
    onAutoIgnoreIssueType,
    onActivateVersion,
    onUninstallVersion
  }: {
    status: PluginIndexSummary | null;
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
    onInstallMarketplacePlugin?: (pluginId: string) => void | Promise<void>;
    onStartPurchaseVerification?: (email: string) => void | Promise<void>;
    onPollPendingPurchaseConnection?: () => void | Promise<void>;
    onRefreshPurchases?: () => void | Promise<void>;
    onDisconnectPurchases?: () => void | Promise<void>;
    onRetryFailure?: (path: string) => void | Promise<void>;
    onOpenFailure?: (path: string) => void | Promise<void>;
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
  let selectedAttentionIssueCode = $state<string | null>(null);
  let selectedIgnoredIssueCode = $state<string | null>(null);
  let openIssueDetails = $state<Record<string, boolean>>({});
  let showIgnoredIssues = $state(false);
  let pluginsDialogElement = $state<HTMLElement>();
  let pendingRetryPaths = $state<Record<string, boolean>>({});
  let pendingOpenPaths = $state<Record<string, boolean>>({});
  let pendingRevealPaths = $state<Record<string, boolean>>({});
  let pendingUnignorePaths = $state<Record<string, boolean>>({});
  let pendingVersionActivations = $state<Record<string, boolean>>({});
  let pendingVersionUninstalls = $state<Record<string, boolean>>({});
  let pendingPluginToggles = $state<Record<string, boolean>>({});
  let pendingIssueTypeActions = $state<Record<string, boolean>>({});
  let hiddenIgnoredPaths = $state<Record<string, boolean>>({});
  let hiddenRetriedPaths = $state<Record<string, string>>({});
  let issueCountsByPlugin = $state<Record<string, PluginIssueCount[]>>({});
  let selectedAttentionIssueItems = $state<PluginIssue[]>([]);
  let selectedIgnoredIssueItems = $state<PluginIssue[]>([]);
  let installStatus = $state<'ready' | 'installing' | 'success' | 'failed'>('ready');
  let installMessage = $state('');
  let installDropActive = $state(false);
  let pendingMarketplaceInstalls = $state<Record<string, boolean>>({});
  let purchasesActionPending = $state<'start' | 'poll' | 'refresh' | 'disconnect' | null>(null);
  let purchaseEmail = $state('');
  let editingPendingPurchaseEmail = $state(false);
  let pendingConfirmation = $state<PendingConfirmation | null>(null);
  let missingDownloadUrlRefreshAttempted = $state(false);
  let issueCountsRequestId = 0;
  let attentionIssuesRequestId = 0;
  let ignoredIssuesRequestId = 0;

  $effect(() => {
    currentPage = initialPage;
  });

  $effect(() => {
    if (purchaseConnection.pending_email) {
      purchaseEmail = purchaseConnection.pending_email;
      return;
    }
    if (!purchaseEmail && purchaseConnection.email) {
      purchaseEmail = purchaseConnection.email;
    }
  });

  $effect(() => {
    if (purchaseConnection.state !== 'pending') {
      editingPendingPurchaseEmail = false;
    }
  });

  $effect(() => {
    if (purchaseConnection.state !== 'connected') {
      missingDownloadUrlRefreshAttempted = false;
      return;
    }
    if (missingDownloadUrlRefreshAttempted || purchasesActionPending || !onRefreshPurchases) return;
    if (!marketplacePlugins.some((plugin) => plugin.owned && !plugin.download_url)) return;

    missingDownloadUrlRefreshAttempted = true;
    void runPurchaseRefresh();
  });

  $effect(() => {
    if (selectedPluginId) {
      internalSelectedPluginId = selectedPluginId;
      currentPage = 'installed';
    }
  });

  $effect(() => {
    const visibleIssues = [...selectedAttentionIssueItems, ...selectedIgnoredIssueItems];
    const nextHiddenRetriedPaths: Record<string, string> = {};
    for (const issue of visibleIssues) {
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
  const purchaseConnection = $derived<PurchaseConnectionSummary>(
    status?.purchase_connection ?? {
      state: 'not_connected',
      email: null,
      pending_email: null,
      pending_expires_at: null,
      last_synced_at: null,
      has_cached_entitlements: false,
      status_message: null,
      storage_warning: null
    }
  );
  const marketplacePlugins = $derived<MarketplacePluginSummary[]>(status?.marketplace_plugins ?? []);
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
  const installedPluginById = $derived.by(() => {
    const grouped = new Map<string, InstalledPluginInfo[]>();
    for (const plugin of installedPlugins) {
      const versions = grouped.get(plugin.id);
      if (versions) versions.push(plugin);
      else grouped.set(plugin.id, [plugin]);
    }
    for (const versions of grouped.values()) {
      versions.sort((left, right) => right.version.localeCompare(left.version, undefined, { numeric: true }));
    }
    return grouped;
  });
  const availableMarketplacePlugins = $derived.by(() => marketplacePlugins);
  const updateMarketplacePlugins = $derived.by(() =>
    marketplacePlugins.filter((plugin) => plugin.owned && marketplaceAction(plugin).action === 'update')
  );
  const autoIgnoredIssueCodes = $derived.by<Set<string>>(() => {
    if (!status || !selectedPluginIdValue) return new Set();
    return new Set(
      status.auto_ignored_issue_types
        .filter((item) => item.plugin_id === selectedPluginIdValue)
        .map((item) => item.error_code)
    );
  });
  const selectedIssueCounts = $derived.by<PluginIssueCount[]>(() => {
    if (!selectedPluginIdValue) return [];
    return issueCountsByPlugin[selectedPluginIdValue] ?? [];
  });
  const activeIssueCount = $derived(selectedSummary?.attention_count ?? 0);
  const activeIssueCategories = $derived.by<IssueCategory[]>(() =>
    buildIssueCategoriesFromCounts(selectedIssueCounts, 'attention')
  );
  const ignoredIssueCategories = $derived.by<IssueCategory[]>(() => {
    if (!showIgnoredIssues) return [];
    return buildIssueCategoriesFromCounts(selectedIssueCounts, 'ignored');
  });
  const selectedAttentionIssues = $derived.by<PluginIssue[]>(() => {
    return selectedAttentionIssueItems.filter((issue) => !hiddenIgnoredPaths[issue.source_path])
      .filter((issue) => hiddenRetriedPaths[issue.source_path] !== issue.last_reported_at);
  });
  const selectedIgnoredIssues = $derived.by<PluginIssue[]>(() => {
    if (!showIgnoredIssues) return [];
    return selectedIgnoredIssueItems;
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
    if (!selectedAttentionIssueCode) return;
    if (activeIssueCategories.some((category) => category.code === selectedAttentionIssueCode)) return;
    selectedAttentionIssueCode = null;
  });

  $effect(() => {
    if (!showIgnoredIssues) {
      selectedIgnoredIssueCode = null;
      return;
    }
    if (!selectedIgnoredIssueCode) return;
    if (ignoredIssueCategories.some((category) => category.code === selectedIgnoredIssueCode)) return;
    selectedIgnoredIssueCode = null;
  });

  $effect(() => {
    if (!selectedPluginIdValue) return;
    void loadIssueCounts(selectedPluginIdValue);
  });

  $effect(() => {
    if (!selectedPluginIdValue || !selectedAttentionIssueCode) {
      selectedAttentionIssueItems = [];
      return;
    }
    void loadAttentionIssues(selectedPluginIdValue, selectedAttentionIssueCode);
  });

  $effect(() => {
    if (!selectedPluginIdValue || !showIgnoredIssues || !selectedIgnoredIssueCode) {
      selectedIgnoredIssueItems = [];
      return;
    }
    void loadIgnoredIssues(selectedPluginIdValue, selectedIgnoredIssueCode);
  });

  function selectPlugin(plugin: InstalledPluginInfo) {
    internalSelectedPluginId = plugin.id;
    selectedAttentionIssueCode = null;
    selectedIgnoredIssueCode = null;
    selectedAttentionIssueItems = [];
    selectedIgnoredIssueItems = [];
    currentPage = 'installed';
  }

  function buildIssueCategoriesFromCounts(
    counts: PluginIssueCount[],
    visibility: 'attention' | 'ignored'
  ): IssueCategory[] {
    if (!selectedPluginIdValue) return [];
    const categories = new Map<string, IssueCategory>();
    for (const item of counts) {
      const isIgnored = item.status === 'ignored';
      if (visibility === 'attention' && isIgnored) continue;
      if (visibility === 'ignored' && !isIgnored) continue;
      const existing = categories.get(item.error_code);
      if (existing) {
        existing.count += item.count;
        existing.autoIgnored = autoIgnoredIssueCodes.has(item.error_code);
        continue;
      }
      categories.set(item.error_code, {
        code: item.error_code,
        label: labelForIssueCode(item.error_code),
        count: item.count,
        pluginId: selectedPluginIdValue,
        autoIgnored: autoIgnoredIssueCodes.has(item.error_code)
      });
    }
    return [...categories.values()].sort(
      (left, right) =>
        right.count - left.count
        || left.label.localeCompare(right.label, undefined, { sensitivity: 'base' })
        || left.code.localeCompare(right.code, undefined, { sensitivity: 'base' })
    );
  }

  async function loadIssueCounts(pluginId: string) {
    const requestId = ++issueCountsRequestId;
    try {
      const counts = await getPluginIssueCounts(pluginId);
      if (requestId !== issueCountsRequestId) return;
      issueCountsByPlugin = { ...issueCountsByPlugin, [pluginId]: counts };
    } catch {
      if (requestId !== issueCountsRequestId) return;
      issueCountsByPlugin = { ...issueCountsByPlugin, [pluginId]: [] };
    }
  }

  async function loadAttentionIssues(pluginId: string, errorCode: string) {
    const requestId = ++attentionIssuesRequestId;
    try {
      const issues = await getPluginIssues(pluginId, null, errorCode, 25);
      if (requestId !== attentionIssuesRequestId) return;
      selectedAttentionIssueItems = issues.filter((issue) => issue.status !== 'ignored');
    } catch {
      if (requestId !== attentionIssuesRequestId) return;
      selectedAttentionIssueItems = [];
    }
  }

  async function loadIgnoredIssues(pluginId: string, errorCode: string) {
    const requestId = ++ignoredIssuesRequestId;
    try {
      const issues = await getPluginIssues(pluginId, 'ignored', errorCode, 25);
      if (requestId !== ignoredIssuesRequestId) return;
      selectedIgnoredIssueItems = issues;
    } catch {
      if (requestId !== ignoredIssuesRequestId) return;
      selectedIgnoredIssueItems = [];
    }
  }

  async function refreshIssueData(pluginId?: string | null) {
    if (!pluginId) return;
    await loadIssueCounts(pluginId);
    if (selectedAttentionIssueCode) await loadAttentionIssues(pluginId, selectedAttentionIssueCode);
    if (showIgnoredIssues && selectedIgnoredIssueCode) await loadIgnoredIssues(pluginId, selectedIgnoredIssueCode);
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

  function toggleAttentionIssueCategory(category: IssueCategory) {
    selectedAttentionIssueCode = selectedAttentionIssueCode === category.code ? null : category.code;
  }

  function toggleIgnoredIssueCategory(category: IssueCategory) {
    selectedIgnoredIssueCode = selectedIgnoredIssueCode === category.code ? null : category.code;
  }

  async function queueRetry(path: string, lastReportedAt: string) {
    if (!onRetryFailure || pendingRetryPaths[path]) return;
    pendingRetryPaths = markPending(pendingRetryPaths, path, true);
    hiddenRetriedPaths = markHiddenRetry(hiddenRetriedPaths, path, lastReportedAt);
    try {
      await onRetryFailure(path);
      await refreshIssueData(selectedPluginIdValue);
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

  async function openIssue(path: string) {
    if (!onOpenFailure || pendingOpenPaths[path]) return;
    pendingOpenPaths = markPending(pendingOpenPaths, path, true);
    try {
      await onOpenFailure(path);
    } finally {
      pendingOpenPaths = markPending(pendingOpenPaths, path, false);
    }
  }

  async function ignoreIssue(path: string, pluginId: string) {
    if (!onIgnoreFailure) return;
    hiddenIgnoredPaths = markPending(hiddenIgnoredPaths, path, true);
    try {
      await onIgnoreFailure(path, pluginId);
      await refreshIssueData(pluginId);
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
      await refreshIssueData(pluginId);
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
      await refreshIssueData(category.pluginId);
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
      await refreshIssueData(category.pluginId);
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
      await refreshIssueData(category.pluginId);
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
    const confirmed = await requestConfirmation({
      title: 'Uninstall plugin version',
      message: `Uninstall plugin version ${version}?`,
      confirmLabel: 'Uninstall',
      confirmTone: 'danger'
    });
    if (!confirmed) return;
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

  function installedVersionsForMarketplacePlugin(pluginId: string) {
    return installedPluginById.get(pluginId) ?? [];
  }

  function marketplaceAction(plugin: MarketplacePluginSummary) {
    const activeInstalled = activeInstalledMarketplaceVersion(plugin);
    if (purchaseConnection.state === 'expired') {
      return { action: 'reconnect', label: 'Reconnect purchases' } as const;
    }
    if (purchaseConnection.state === 'pending') {
      return { action: 'pending', label: 'Waiting for verification' } as const;
    }
    if (purchaseConnection.state !== 'connected') {
      return { action: 'connect', label: 'Connect purchases' } as const;
    }
    if (!plugin.owned) {
      return { action: 'buy', label: 'Buy' } as const;
    }
    if (!plugin.download_url) {
      return { action: 'website', label: 'Open website' } as const;
    }
    if (!activeInstalled) {
      return { action: 'install', label: 'Install plugin' } as const;
    }
    if (!plugin.latest_version) {
      return { action: 'reinstall', label: 'Reinstall' } as const;
    }
    const compare = activeInstalled.version.localeCompare(plugin.latest_version, undefined, { numeric: true });
    if (compare < 0) {
      return { action: 'update', label: 'Update' } as const;
    }
    return { action: 'reinstall', label: 'Reinstall' } as const;
  }

  function activeInstalledMarketplaceVersion(plugin: MarketplacePluginSummary) {
    const installedVersions = installedVersionsForMarketplacePlugin(plugin.plugin_id);
    return installedVersions.find((item) => item.is_active) ?? installedVersions[0] ?? null;
  }

  function marketplaceBadges(plugin: MarketplacePluginSummary) {
    const badges: Array<{ label: string; tone: MarketplaceBadgeTone }> = [];
    const installed = activeInstalledMarketplaceVersion(plugin);
    const relation = marketplaceVersionRelation(plugin);
    if (plugin.owned) badges.push({ label: 'Owned', tone: 'owned' });
    if (installed) badges.push({ label: 'Installed', tone: 'installed' });
    if (plugin.owned && !installed) badges.push({ label: 'Not installed', tone: 'neutral' });
    if (relation === 'behind') badges.push({ label: 'Update available', tone: 'update' });
    if (relation === 'ahead') badges.push({ label: 'Ahead of catalog', tone: 'development' });
    if (installed && isDevelopmentVersion(installed.version)) badges.push({ label: 'Development build', tone: 'development' });
    return badges;
  }

  function marketplaceActionIsPrimary(plugin: MarketplacePluginSummary) {
    const action = marketplaceAction(plugin).action;
    return action === 'install' || action === 'reinstall' || action === 'update';
  }

  function marketplaceActionIsDisabled(plugin: MarketplacePluginSummary) {
    const action = marketplaceAction(plugin).action;
    return action === 'pending';
  }

  function compareVersions(installedVersion: string, catalogVersion: string) {
    return installedVersion.localeCompare(catalogVersion, undefined, { numeric: true });
  }

  function isDevelopmentVersion(version: string) {
    return /(?:^|[.-])(dev|alpha|beta|rc|preview)(?:[.-]|$)/i.test(version);
  }

  function marketplaceVersionRelation(plugin: MarketplacePluginSummary) {
    const installed = activeInstalledMarketplaceVersion(plugin);
    if (!installed) return 'not_installed' as const;
    if (!plugin.latest_version) return 'no_catalog' as const;
    const compare = compareVersions(installed.version, plugin.latest_version);
    if (compare > 0) return 'ahead' as const;
    if (compare < 0) return 'behind' as const;
    return 'current' as const;
  }

  function installedVersionLabel(plugin: MarketplacePluginSummary) {
    const installed = activeInstalledMarketplaceVersion(plugin);
    if (!installed) return null;
    return installed.version;
  }

  function catalogVersionLabel(plugin: MarketplacePluginSummary) {
    return plugin.latest_version;
  }

  function purchaseStatusTitle() {
    if (purchaseConnection.state === 'connected') return 'Purchases connected';
    if (purchaseConnection.state === 'pending') return 'Verification email sent';
    if (purchaseConnection.state === 'expired') return 'Verification expired';
    return 'Connect your Searchmonkey purchases';
  }

  function formatRelativeConnectionDate(value: string | null | undefined) {
    if (!value) return null;
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return null;
    return new Intl.DateTimeFormat(undefined, {
      day: 'numeric',
      month: 'short',
      year: 'numeric',
      hour: 'numeric',
      minute: '2-digit'
    }).format(date);
  }

  function showPendingPurchaseForm() {
    editingPendingPurchaseEmail = true;
  }

  async function startEmailVerification() {
    if (!onStartPurchaseVerification || purchasesActionPending) return;
    const email = purchaseEmail.trim();
    if (!email) return;
    purchasesActionPending = 'start';
    try {
      await onStartPurchaseVerification(email);
    } finally {
      purchasesActionPending = null;
    }
  }

  async function openPurchaseLink(plugin: MarketplacePluginSummary) {
    await openUrl(plugin.buy_url ?? plugin.homepage_url ?? PLUGINS_URL);
  }

  async function openPluginCatalog() {
    await openUrl(PLUGINS_URL);
  }

  async function runPurchaseRefresh() {
    if (!onRefreshPurchases || purchasesActionPending) return;
    purchasesActionPending = 'refresh';
    try {
      await onRefreshPurchases();
    } finally {
      purchasesActionPending = null;
    }
  }

  async function runPurchasePoll() {
    if (!onPollPendingPurchaseConnection || purchasesActionPending) return;
    purchasesActionPending = 'poll';
    try {
      await onPollPendingPurchaseConnection();
    } finally {
      purchasesActionPending = null;
    }
  }

  async function runPurchaseDisconnect() {
    if (!onDisconnectPurchases || purchasesActionPending) return;
    purchasesActionPending = 'disconnect';
    try {
      await onDisconnectPurchases();
    } finally {
      purchasesActionPending = null;
    }
  }

  async function runMarketplaceAction(plugin: MarketplacePluginSummary) {
    const decision = marketplaceAction(plugin);
    if (decision.action === 'connect' || decision.action === 'reconnect') {
      return;
    }
    if (decision.action === 'buy') {
      await openPurchaseLink(plugin);
      return;
    }
    if (decision.action === 'website') {
      await openPurchaseLink(plugin);
      return;
    }
    if (decision.action === 'pending' || !onInstallMarketplacePlugin) return;
    if (pendingMarketplaceInstalls[plugin.plugin_id]) return;
    pendingMarketplaceInstalls = { ...pendingMarketplaceInstalls, [plugin.plugin_id]: true };
    try {
      await onInstallMarketplacePlugin(plugin.plugin_id);
    } finally {
      pendingMarketplaceInstalls = { ...pendingMarketplaceInstalls, [plugin.plugin_id]: false };
    }
  }

  async function refreshSelectedPlugin() {
    if (!selectedPlugin || !onRefreshPlugin) return;
    await onRefreshPlugin(selectedPlugin.id);
  }

  async function resetSelectedPlugin() {
    if (!selectedPlugin || !onResetPlugin) return;
    const confirmed = await requestConfirmation({
      title: 'Reset plugin cache',
      message: `Reset cached output for ${selectedPlugin.name}?`,
      confirmLabel: 'Reset',
      confirmTone: 'danger'
    });
    if (!confirmed) return;
    await onResetPlugin(selectedPlugin.id);
  }

  async function toggleSelectedPluginEnabled() {
    if (!selectedPlugin || !onSetPluginEnabled) return;
    const nextEnabled = !selectedPlugin.enabled;
    if (!nextEnabled) {
      const confirmed = await requestConfirmation({
        title: 'Disable plugin',
        message: `Disable ${selectedPlugin.name}? This also clears its cached output so it stops affecting search results.`,
        confirmLabel: 'Disable',
        confirmTone: 'danger'
      });
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

  function requestConfirmation(
    options: Omit<PendingConfirmation, 'resolve'>
  ): Promise<boolean> {
    if (pendingConfirmation) {
      pendingConfirmation.resolve(false);
    }
    return new Promise((resolve) => {
      pendingConfirmation = { ...options, resolve };
    });
  }

  function closeConfirmation(confirmed: boolean) {
    if (!pendingConfirmation) return;
    const { resolve } = pendingConfirmation;
    pendingConfirmation = null;
    resolve(confirmed);
  }
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
        <button type="button" class:active={currentPage === 'available'} onclick={() => setPage('available')}>Library</button>
        <button type="button" class:active={currentPage === 'updates'} onclick={() => setPage('updates')}>Updates</button>
        <button type="button" class:active={currentPage === 'install'} onclick={() => setPage('install')}>Install from file…</button>
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
        <div class="plugin-content marketplace-page">
          <header class="detail-header">
            <div>
              <h3>Library</h3>
              <p class="muted">Browse, install and update plugins connected to your Searchmonkey purchases.</p>
            </div>
          </header>

          <section class:compact={purchaseConnection.state === 'connected'} class="panel purchase-panel">
            <div class="purchase-copy">
              {#if purchaseConnection.state === 'connected'}
                <strong>Connected as {purchaseConnection.email ?? 'your account'}</strong>
                <div class="purchase-meta">
                  {#if formatRelativeConnectionDate(purchaseConnection.last_synced_at)}
                    <span>Last synced {formatRelativeConnectionDate(purchaseConnection.last_synced_at)}</span>
                  {/if}
                  <span>Secure purchase verification via searchmonkey.dev</span>
                </div>
              {:else if purchaseConnection.state === 'pending' && !editingPendingPurchaseEmail}
                <strong>{purchaseStatusTitle()}</strong>
                <span class="purchase-email-display">{purchaseConnection.pending_email ?? purchaseEmail}</span>
                <p class="muted">Check your inbox and click the verification link.</p>
                <p class="purchase-trust">Secure purchase verification via searchmonkey.dev</p>
              {:else if purchaseConnection.state === 'expired'}
                <strong>{purchaseStatusTitle()}</strong>
                <p class="muted">The verification link expired. Enter the checkout email to send a new one.</p>
                <p class="purchase-trust">Secure purchase verification via searchmonkey.dev</p>
              {:else}
                <strong>{purchaseStatusTitle()}</strong>
                <p class="muted">Connect your Searchmonkey purchases to install and update linked plugins in the app.</p>
                <p class="purchase-trust">Secure purchase verification via searchmonkey.dev</p>
              {/if}
              {#if purchaseConnection.status_message}
                <p class="muted">{purchaseConnection.status_message}</p>
              {/if}
              {#if purchaseConnection.storage_warning}
                <p class="muted">{purchaseConnection.storage_warning}</p>
              {/if}
            </div>
            <div class="purchase-actions">
              {#if purchaseConnection.state === 'connected'}
                <button type="button" class="secondary" disabled={purchasesActionPending !== null} onclick={runPurchaseRefresh}>
                  {purchasesActionPending === 'refresh' ? 'Refreshing…' : 'Refresh'}
                </button>
                <button type="button" class="secondary" onclick={openPluginCatalog}>Browse plugins</button>
                <button type="button" class="tertiary" disabled={purchasesActionPending !== null} onclick={runPurchaseDisconnect}>
                  {purchasesActionPending === 'disconnect' ? 'Disconnecting…' : 'Disconnect'}
                </button>
              {:else}
                {#if purchaseConnection.state === 'pending' && !editingPendingPurchaseEmail}
                  <button type="button" class="secondary" disabled={purchasesActionPending !== null} onclick={startEmailVerification}>
                    {purchasesActionPending === 'start' ? 'Sending…' : 'Resend email'}
                  </button>
                  <button type="button" class="tertiary" disabled={purchasesActionPending !== null} onclick={showPendingPurchaseForm}>
                    Change email
                  </button>
                {:else}
                  <label class="purchase-email-field">
                    <span>Email used at checkout</span>
                    <input
                      type="email"
                      bind:value={purchaseEmail}
                      placeholder="buyer@example.com"
                      autocomplete="email"
                    />
                  </label>
                  <button type="button" class="primary-action purchase-submit" disabled={purchasesActionPending !== null || !purchaseEmail.trim()} onclick={startEmailVerification}>
                    {purchaseConnection.state === 'expired'
                      ? purchasesActionPending === 'start' ? 'Sending…' : 'Reconnect purchases'
                      : purchasesActionPending === 'start' ? 'Sending…' : 'Connect purchases'}
                  </button>
                  <button type="button" class="secondary" onclick={openPluginCatalog}>Browse plugins</button>
                {/if}
              {/if}
            </div>
          </section>

          {#if availableMarketplacePlugins.length}
            <section class="marketplace-list">
              {#each availableMarketplacePlugins as plugin}
                <article class="panel marketplace-card">
                  <div class="marketplace-copy">
                    <div class="marketplace-heading">
                      <h4>{plugin.name}</h4>
                    </div>
                    <div class="marketplace-version-list">
                      {#if installedVersionLabel(plugin)}
                        <div class="marketplace-version-row">
                          <span class="marketplace-version-label">Installed locally</span>
                          <span>{installedVersionLabel(plugin)}</span>
                        </div>
                      {/if}
                      {#if catalogVersionLabel(plugin)}
                        <div class="marketplace-version-row">
                          <span class="marketplace-version-label">Catalog version</span>
                          <span>{catalogVersionLabel(plugin)}</span>
                        </div>
                      {/if}
                    </div>
                    <div class="marketplace-badges">
                      {#each marketplaceBadges(plugin) as badge}
                        <span class={`state-badge ${badge.tone}`}>{badge.label}</span>
                      {/each}
                    </div>
                  </div>
                  <div class="marketplace-actions">
                    <button
                      type="button"
                      class:secondary={!marketplaceActionIsPrimary(plugin)}
                      class:primary-action={marketplaceActionIsPrimary(plugin)}
                      disabled={pendingMarketplaceInstalls[plugin.plugin_id] || marketplaceActionIsDisabled(plugin)}
                      onclick={() => runMarketplaceAction(plugin)}
                    >
                      {pendingMarketplaceInstalls[plugin.plugin_id] ? 'Installing…' : marketplaceAction(plugin).label}
                    </button>
                  </div>
                </article>
              {/each}
            </section>
          {:else if purchaseConnection.state === 'connected'}
            <div class="empty-state plugin-content">
              <h3>No plugins available</h3>
              <p>Your connected library has no plugins to install right now.</p>
              <div class="empty-actions">
                <button type="button" class="secondary" onclick={openPluginCatalog}>Browse plugins</button>
              </div>
            </div>
          {/if}
        </div>
      {:else if currentPage === 'updates'}
        <div class="plugin-content marketplace-page">
          <header class="detail-header">
            <div>
              <h3>Updates</h3>
              <p class="muted">Installed plugins with newer versions available from your library.</p>
            </div>
          </header>

          <section class="panel purchase-panel compact">
            <div class="purchase-copy">
              {#if purchaseConnection.state === 'connected'}
                <strong>Connected as {purchaseConnection.email ?? 'your account'}</strong>
                <div class="purchase-meta">
                  {#if formatRelativeConnectionDate(purchaseConnection.last_synced_at)}
                    <span>Last synced {formatRelativeConnectionDate(purchaseConnection.last_synced_at)}</span>
                  {/if}
                  <span>Secure purchase verification via searchmonkey.dev</span>
                </div>
              {:else if purchaseConnection.state === 'pending'}
                <strong>Verification email sent</strong>
                <span class="purchase-email-display">{purchaseConnection.pending_email ?? purchaseEmail}</span>
              {:else if purchaseConnection.state === 'expired'}
                <strong>Verification expired</strong>
              {:else}
                <strong>Connect purchases</strong>
              {/if}
            </div>
            <div class="purchase-actions">
              {#if purchaseConnection.state === 'connected'}
                <button type="button" class="secondary" disabled={purchasesActionPending !== null} onclick={runPurchaseRefresh}>
                  {purchasesActionPending === 'refresh' ? 'Refreshing…' : 'Refresh'}
                </button>
                <button type="button" class="secondary" onclick={openPluginCatalog}>Browse plugins</button>
                <button type="button" class="tertiary" disabled={purchasesActionPending !== null} onclick={runPurchaseDisconnect}>
                  {purchasesActionPending === 'disconnect' ? 'Disconnecting…' : 'Disconnect'}
                </button>
              {:else if purchaseConnection.state === 'pending'}
                {#if !editingPendingPurchaseEmail}
                  <button type="button" class="secondary" disabled={purchasesActionPending !== null} onclick={startEmailVerification}>
                    {purchasesActionPending === 'start' ? 'Sending…' : 'Resend email'}
                  </button>
                  <button type="button" class="tertiary" disabled={purchasesActionPending !== null} onclick={showPendingPurchaseForm}>
                    Change email
                  </button>
                {:else}
                  <label class="purchase-email-field">
                    <span>Email used at checkout</span>
                    <input
                      type="email"
                      bind:value={purchaseEmail}
                      placeholder="buyer@example.com"
                      autocomplete="email"
                    />
                  </label>
                  <button type="button" class="primary-action purchase-submit" disabled={purchasesActionPending !== null || !purchaseEmail.trim()} onclick={startEmailVerification}>
                    {purchasesActionPending === 'start' ? 'Sending…' : 'Connect purchases'}
                  </button>
                {/if}
              {:else}
                <label class="purchase-email-field">
                  <span>Email used at checkout</span>
                  <input
                    type="email"
                    bind:value={purchaseEmail}
                    placeholder="buyer@example.com"
                    autocomplete="email"
                  />
                </label>
                <button type="button" class="primary-action purchase-submit" disabled={purchasesActionPending !== null || !purchaseEmail.trim()} onclick={startEmailVerification}>
                  {purchaseConnection.state === 'expired'
                    ? purchasesActionPending === 'start' ? 'Sending…' : 'Reconnect purchases'
                    : purchasesActionPending === 'start' ? 'Sending…' : 'Connect purchases'}
                </button>
              {/if}
            </div>
          </section>

          {#if updateMarketplacePlugins.length}
            <section class="marketplace-list">
              {#each updateMarketplacePlugins as plugin}
                <article class="panel marketplace-card">
                  <div class="marketplace-copy">
                    <div class="marketplace-heading">
                      <h4>{plugin.name}</h4>
                    </div>
                    <div class="marketplace-version-list">
                      {#if installedVersionLabel(plugin)}
                        <div class="marketplace-version-row">
                          <span class="marketplace-version-label">Installed locally</span>
                          <span>{installedVersionLabel(plugin)}</span>
                        </div>
                      {/if}
                      {#if catalogVersionLabel(plugin)}
                        <div class="marketplace-version-row">
                          <span class="marketplace-version-label">Catalog version</span>
                          <span>{catalogVersionLabel(plugin)}</span>
                        </div>
                      {/if}
                    </div>
                    <div class="marketplace-badges">
                      {#each marketplaceBadges(plugin) as badge}
                        <span class={`state-badge ${badge.tone}`}>{badge.label}</span>
                      {/each}
                    </div>
                  </div>
                  <div class="marketplace-actions">
                    <button
                      type="button"
                      class="primary-action"
                      disabled={pendingMarketplaceInstalls[plugin.plugin_id]}
                      onclick={() => runMarketplaceAction(plugin)}
                    >
                      {pendingMarketplaceInstalls[plugin.plugin_id] ? 'Installing…' : 'Update'}
                    </button>
                  </div>
                </article>
              {/each}
            </section>
          {:else}
            <div class="empty-state plugin-content">
              <h3>No Updates</h3>
              <p>
                {purchaseConnection.state === 'connected'
                  ? 'Purchased plugins are up to date.'
                  : 'Connect purchases to check for plugin updates.'}
              </p>
            </div>
          {/if}
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
                {#if activeIssueCount}
                  {activeIssueCount} files need attention
                {:else}
                  No active indexing problems
                {/if}
                {#if ignoredIssueCount} · {ignoredIssueCount} ignored{/if}
              </p>
            </div>
          </div>

          {#if activeIssueCategories.length}
            <div class="issues-section">
              <div class="issues-section-header">
                <h5>Needs Attention</h5>
                <p class="muted">Only active issue types are shown here.</p>
              </div>
              <div class="issue-categories attention-categories">
                {#each activeIssueCategories as category}
                  <div class="issue-category-group">
                    <button
                      type="button"
                      class:attention-pill={true}
                      class:selected={selectedAttentionIssueCode === category.code}
                      class="issue-category-pill"
                      onclick={() => toggleAttentionIssueCategory(category)}
                    >
                      <span>{category.label}</span>
                      <strong>{category.count}</strong>
                      <span class="chevron">{selectedAttentionIssueCode === category.code ? '▴' : '▾'}</span>
                    </button>
                    {#if selectedAttentionIssueCode === category.code}
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

              {#if selectedAttentionIssues.length}
                <div class="issues-list">
                  {#each selectedAttentionIssues as issue}
                    <article class="issue-card">
                      <div class="issue-copy">
                        <strong title={issue.file_name}>{truncateFilenameMiddle(issue.file_name)}</strong>
                        <p>{labelForIssue(issue)}</p>
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
                          disabled={pendingOpenPaths[issue.source_path]}
                          onclick={() => openIssue(issue.source_path)}
                        >
                          {pendingOpenPaths[issue.source_path] ? 'Opening…' : 'Open'}
                        </button>
                        <button
                          type="button"
                          class="secondary"
                          disabled={pendingRevealPaths[issue.source_path]}
                          onclick={() => revealIssue(issue.source_path)}
                        >
                          {pendingRevealPaths[issue.source_path] ? 'Revealing…' : 'Reveal'}
                        </button>
                        <button type="button" class="secondary" onclick={() => ignoreIssue(issue.source_path, issue.plugin_id)}>
                          Ignore
                        </button>
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
              {/if}
            </div>
          {:else}
            <div class="issues-empty-state">
              <p class="issues-empty-title">No files need attention</p>
              {#if ignoredIssueCount}
                <p class="muted">{ignoredIssueCount} issues are auto-handled</p>
              {/if}
            </div>
          {/if}

          {#if ignoredIssueCount}
            <details class="ignored-issues-panel" bind:open={showIgnoredIssues}>
              <summary>
                <span class="ignored-summary-label">
                  <span>Ignored &amp; Auto-handled</span>
                  <span class="summary-chevron">{showIgnoredIssues ? '▴' : '▾'}</span>
                </span>
                <strong>{ignoredIssueCount}</strong>
              </summary>

              {#if showIgnoredIssues}
                {#if ignoredIssueCategories.length}
                  <div class="issues-section ignored-section">
                    <div class="issues-section-header">
                      <h5>Ignored Issue Types</h5>
                      <p class="muted">Configuration and muted issue types live here.</p>
                    </div>
                    <div class="issue-categories ignored-categories">
                      {#each ignoredIssueCategories as category}
                        <div class="issue-category-group">
                          <button
                            type="button"
                            class:subtle-pill={true}
                            class:selected={selectedIgnoredIssueCode === category.code}
                            class="issue-category-pill"
                            onclick={() => toggleIgnoredIssueCategory(category)}
                          >
                            <span>{category.label}</span>
                            <strong>{category.count}</strong>
                            <span class="chevron">{selectedIgnoredIssueCode === category.code ? '▴' : '▾'}</span>
                          </button>
                          {#if selectedIgnoredIssueCode === category.code}
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

                    {#if selectedIgnoredIssues.length}
                      <div class="issues-list">
                        {#each selectedIgnoredIssues as issue}
                          <article class="issue-card ignored-card">
                            <div class="issue-copy">
                              <strong title={issue.file_name}>{truncateFilenameMiddle(issue.file_name)}</strong>
                              <p>
                                {labelForIssue(issue)}
                                <span class="ignored-badge">Ignored</span>
                              </p>
                              {#if issueStatusText(issue)}
                                <p class="muted">{issueStatusText(issue)}</p>
                              {/if}
                            </div>
                            <div class="issue-actions">
                              <button
                                type="button"
                                class="secondary"
                                disabled={pendingOpenPaths[issue.source_path]}
                                onclick={() => openIssue(issue.source_path)}
                              >
                                {pendingOpenPaths[issue.source_path] ? 'Opening…' : 'Open'}
                              </button>
                              <button
                                type="button"
                                class="secondary"
                                disabled={pendingRevealPaths[issue.source_path]}
                                onclick={() => revealIssue(issue.source_path)}
                              >
                                {pendingRevealPaths[issue.source_path] ? 'Revealing…' : 'Reveal'}
                              </button>
                              <button
                                type="button"
                                class="secondary"
                                disabled={pendingUnignorePaths[issue.source_path]}
                                onclick={() => unignoreIssue(issue.source_path, issue.plugin_id)}
                              >
                                {pendingUnignorePaths[issue.source_path] ? 'Re-enabling…' : 'Re-enable issue'}
                              </button>
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
                    {/if}
                  </div>
                {:else}
                  <p class="muted">No ignored issue types.</p>
                {/if}
              {/if}
            </details>
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

    {#if pendingConfirmation}
      <div class="confirm-overlay" role="presentation">
        <div class="confirm-dialog" role="alertdialog" aria-modal="true" aria-labelledby="plugin-confirm-title">
          <div class="confirm-copy">
            <h3 id="plugin-confirm-title">{pendingConfirmation.title}</h3>
            <p>{pendingConfirmation.message}</p>
          </div>
          <div class="confirm-actions">
            <button type="button" class="secondary" onclick={() => closeConfirmation(false)}>Cancel</button>
            <button
              type="button"
              class:danger={pendingConfirmation.confirmTone === 'danger'}
              onclick={() => closeConfirmation(true)}
            >
              {pendingConfirmation.confirmLabel}
            </button>
          </div>
        </div>
      </div>
    {/if}
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

  .confirm-overlay {
    position: absolute;
    inset: 0;
    z-index: 3;
    display: grid;
    place-items: center;
    padding: 24px;
    background: rgba(247, 250, 247, 0.68);
    backdrop-filter: blur(2px);
  }

  .confirm-dialog {
    display: grid;
    gap: 18px;
    width: min(420px, 100%);
    padding: 22px;
    border: 1px solid #d9e0d9;
    border-radius: 14px;
    background: #ffffff;
    box-shadow: 0 18px 36px rgba(27, 35, 42, 0.16);
  }

  .confirm-copy {
    display: grid;
    gap: 8px;
  }

  .confirm-copy h3,
  .confirm-copy p {
    margin: 0;
  }

  .confirm-copy p {
    color: #52606b;
    line-height: 1.5;
  }

  .confirm-actions {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
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

  .marketplace-page,
  .marketplace-list {
    display: grid;
    gap: 14px;
  }

  .purchase-panel {
    display: grid;
    gap: 16px;
  }

  .purchase-panel.compact {
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    padding-top: 14px;
    padding-bottom: 14px;
  }

  .purchase-copy {
    display: grid;
    gap: 6px;
  }

  .purchase-email-display {
    color: #1c232b;
    font-size: 18px;
    font-weight: 620;
    line-height: 1.2;
  }

  .purchase-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 14px;
    color: #6c7680;
    font-size: 12px;
  }

  .purchase-trust {
    margin: 0;
    color: #72808a;
    font-size: 12px;
  }

  .purchase-email-field {
    display: grid;
    gap: 6px;
    min-width: min(100%, 360px);
  }

  .purchase-email-field span {
    font-size: 12px;
    color: #66717d;
  }

  .purchase-email-field input {
    width: 100%;
    height: 44px;
    padding: 0 14px;
    border: 1px solid #d9e0d9;
    border-radius: 12px;
    background: #fff;
    color: #1c232b;
  }

  .purchase-actions,
  .marketplace-actions {
    display: flex;
    gap: 10px;
    align-items: center;
    flex-wrap: wrap;
  }

  .purchase-actions {
    justify-content: flex-start;
  }

  .purchase-submit {
    width: min(100%, 360px);
  }

  .marketplace-card {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: end;
    gap: 14px 18px;
    padding: 16px 18px;
  }

  .marketplace-copy {
    display: grid;
    gap: 8px;
    min-width: 0;
  }

  .marketplace-heading {
    display: block;
  }

  .marketplace-heading h4 {
    margin: 0;
  }

  .marketplace-version-list {
    display: grid;
    gap: 4px;
  }

  .marketplace-version-row {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: baseline;
    color: #44515c;
    font-size: 13px;
  }

  .marketplace-version-label {
    color: #6c7782;
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .marketplace-badges {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .state-badge {
    display: inline-flex;
    align-items: center;
    min-height: 24px;
    padding: 0 10px;
    border-radius: 999px;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.03em;
    text-transform: uppercase;
  }

  .state-badge.owned {
    border: 1px solid #d5dae9;
    background: #f3f5fb;
    color: #485777;
  }

  .state-badge.installed {
    border: 1px solid #c6dccd;
    background: #edf7f1;
    color: #0f6b3b;
  }

  .state-badge.update {
    border: 1px solid #e8d2a8;
    background: #fff6e6;
    color: #8d5a00;
  }

  .state-badge.development {
    border: 1px solid #dbc8f2;
    background: #f6f0ff;
    color: #69439b;
  }

  .state-badge.neutral {
    border: 1px solid #d9dee3;
    background: #f5f7f8;
    color: #5f6a74;
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

  .plugin-content::after {
    content: '';
    display: block;
    height: 72px;
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
  .secondary,
  .primary-action,
  .tertiary,
  .confirm-actions button {
    border: 1px solid #d2dcd2;
    border-radius: 8px;
    padding: 8px 12px;
    color: #2c3740;
    background: #fff;
    cursor: pointer;
  }

  .primary-action {
    min-height: 44px;
    padding: 0 18px;
    border-color: #0f6b3b;
    border-radius: 12px;
    color: #ffffff;
    background: #0f6b3b;
    font-weight: 600;
    box-shadow: 0 1px 0 rgba(11, 95, 50, 0.2);
  }

  .primary-action:hover,
  .primary-action:focus-visible {
    background: #0c5a32;
    outline: none;
  }

  .primary-action:disabled {
    border-color: #c7d1cb;
    color: #7e8883;
    background: #dfe5e0;
    cursor: not-allowed;
    box-shadow: none;
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

  .confirm-actions .danger {
    border-color: #d9b9b9;
    background: #7d1f1f;
    color: #fff;
  }

  .tertiary {
    border-color: transparent;
    color: #6a7480;
    background: transparent;
  }

  .tertiary:hover,
  .tertiary:focus-visible {
    border-color: #d9e0d9;
    background: #f6f8f6;
    outline: none;
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

  .issues-section,
  .issues-empty-state {
    display: grid;
    gap: 12px;
  }

  .issues-section-header {
    display: grid;
    gap: 4px;
  }

  .issues-section-header h5,
  .issues-empty-title {
    margin: 0;
    color: #1c232b;
    font-size: 15px;
    font-weight: 700;
  }

  .issues-section-header .muted,
  .issues-empty-state .muted {
    margin: 0;
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

  .issue-category-pill.attention-pill {
    border-color: #cadecb;
    background: #f7fbf8;
  }

  .issue-category-pill.subtle-pill {
    border-color: #d7dde4;
    background: #fff;
    color: #54606c;
  }

  .issue-category-pill.subtle-pill:hover {
    border-color: #c5ced7;
    background: #f7f9fb;
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

  .empty-state {
    justify-items: start;
  }

  .empty-illustration {
    display: grid;
    place-items: center;
    width: 56px;
    height: 56px;
    border: 1px solid #d8dfd8;
    border-radius: 16px;
    background: linear-gradient(180deg, #fbfdfb 0%, #eef5ef 100%);
  }

  .empty-glyph {
    color: #0f6b3b;
    font-size: 26px;
    line-height: 1;
  }

  .empty-steps {
    display: grid;
    gap: 6px;
    margin: 0;
    padding-left: 18px;
    color: #48545f;
  }

  .empty-cta {
    min-width: 220px;
  }

  @media (max-width: 900px) {
    .purchase-panel.compact,
    .marketplace-card {
      grid-template-columns: 1fr;
    }

    .purchase-submit,
    .purchase-email-field,
    .empty-cta {
      width: 100%;
    }
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

  .ignored-issues-panel {
    border-top: 1px solid #e7ece7;
    padding-top: 12px;
  }

  .ignored-issues-panel summary {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    cursor: pointer;
    color: #33404b;
    font-weight: 600;
    list-style: none;
  }

  .ignored-summary-label {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }

  .summary-chevron {
    color: #6a7580;
    font-size: 13px;
    line-height: 1;
  }

  .ignored-issues-panel summary::-webkit-details-marker {
    display: none;
  }

  .ignored-issues-panel summary strong {
    color: #5e6974;
    font-size: 13px;
  }

  .ignored-section {
    margin-top: 12px;
    padding: 14px;
    border: 1px solid #e7ece7;
    border-radius: 12px;
    background: #f8faf8;
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
