<script lang="ts">
  import type {
    InstalledPluginInfo,
    PluginHealthSummary,
    PluginIndexStatus,
    PluginIssue
  } from '$lib/types';

  let {
    status,
    selectedPluginId = null,
    onClose,
    onRefresh,
    onOpenFolder,
    onTogglePaused,
    onRebuild,
    onRetryFailure,
    onRevealFailure
  }: {
    status: PluginIndexStatus | null;
    selectedPluginId?: string | null;
    onClose?: () => void;
    onRefresh?: () => void;
    onOpenFolder?: () => void;
    onTogglePaused?: () => void;
    onRebuild?: () => void;
    onRetryFailure?: (path: string) => void;
    onRevealFailure?: (path: string) => void;
  } = $props();

  let internalSelectedPluginId = $state<string | null>(null);
  let selectedIssueCode = $state<string | null>(null);

  $effect(() => {
    if (selectedPluginId) {
      internalSelectedPluginId = selectedPluginId;
    }
  });

  const installedPlugins = $derived(status?.installed_plugins ?? []);
  const selectedPlugin = $derived.by(() => {
    if (!installedPlugins.length) return null;
    if (internalSelectedPluginId) {
      return installedPlugins.find((plugin) => plugin.id === internalSelectedPluginId) ?? installedPlugins[0];
    }
    return installedPlugins[0];
  });
  const selectedSummary = $derived.by<PluginHealthSummary | null>(() => {
    if (!status || !selectedPlugin) return null;
    return status.plugin_summaries.find((summary) => summary.plugin_id === selectedPlugin.id) ?? null;
  });
  const selectedIssues = $derived.by<PluginIssue[]>(() => {
    if (!status || !selectedPlugin) return [];
    let issues = status.issues.filter((issue) => issue.plugin_id === selectedPlugin.id);
    if (selectedIssueCode) {
      issues = issues.filter((issue) => issue.error_code === selectedIssueCode);
    }
    return issues;
  });
  const issueCategories = $derived.by(() => {
    if (!status || !selectedPlugin) return [];
    const counts = new Map<string, { code: string; label: string; count: number }>();
    for (const issue of status.issues) {
      if (issue.plugin_id !== selectedPlugin.id) continue;
      const existing = counts.get(issue.error_code);
      if (existing) {
        existing.count += 1;
        continue;
      }
      counts.set(issue.error_code, {
        code: issue.error_code,
        label: labelForIssue(issue),
        count: 1
      });
    }
    return [...counts.values()].sort((left, right) => right.count - left.count || left.label.localeCompare(right.label));
  });
  const indexingLabel = $derived.by(() => {
    if (!status) return 'Idle';
    if (status.paused) return 'Processing paused';
    if (status.plugin_state === 'working') return 'Working';
    return 'Idle';
  });

  function selectPlugin(plugin: InstalledPluginInfo) {
    internalSelectedPluginId = plugin.id;
    selectedIssueCode = null;
  }

  function labelForIssue(issue: PluginIssue): string {
    switch (issue.error_code) {
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
        return issue.message;
    }
  }

  function retryMessage(retryAfter?: string | null): string | null {
    if (!retryAfter) return null;
    const retryTime = new Date(retryAfter).getTime();
    if (!Number.isFinite(retryTime)) return 'Will retry later';
    const deltaMs = retryTime - Date.now();
    if (deltaMs <= 0) return 'Retry available now';
    const minutes = Math.ceil(deltaMs / 60000);
    if (minutes < 60) return `Retry available in ${minutes} minute${minutes === 1 ? '' : 's'}`;
    const hours = Math.ceil(minutes / 60);
    if (hours < 24) return `Retry available in ${hours} hour${hours === 1 ? '' : 's'}`;
    const days = Math.ceil(hours / 24);
    return `Retry available in ${days} day${days === 1 ? '' : 's'}`;
  }
</script>

<div class="modal-layer" role="presentation">
  <button class="modal-backdrop" type="button" aria-label="Close plugin manager" onclick={onClose}></button>

  <div class="plugins-dialog" role="dialog" aria-modal="true" aria-labelledby="plugins-title">
    <aside class="sidebar">
      <h2 id="plugins-title">Plugins</h2>

      {#if installedPlugins.length}
        <div class="plugin-list">
          {#each installedPlugins as plugin}
            <button
              type="button"
              class:selected={selectedPlugin?.id === plugin.id}
              onclick={() => selectPlugin(plugin)}
            >
              <span>{plugin.name}</span>
              <span class:enabled={plugin.enabled}>{plugin.enabled ? 'Enabled' : 'Disabled'}</span>
            </button>
          {/each}
        </div>
      {/if}
    </aside>

    <section class="detail">
      {#if selectedPlugin}
        <header class="detail-header">
          <div>
            <h3>{selectedPlugin.name}</h3>
            <p>{selectedPlugin.enabled ? 'Enabled' : 'Disabled'}</p>
          </div>

          <details class="menu">
            <summary>More...</summary>
            <div class="menu-panel">
              <button type="button" class="secondary" disabled>{selectedPlugin.enabled ? 'Disable' : 'Enable'}</button>
              <button type="button" class="secondary" disabled>Uninstall</button>
              <button type="button" class="secondary" onclick={onOpenFolder}>Open Plugin Folder</button>
              <button type="button" class="secondary" onclick={onTogglePaused}>
                {status?.paused ? 'Resume Background Processing' : 'Pause Background Processing'}
              </button>
              <button type="button" class="secondary" onclick={onRefresh}>Refresh</button>
              <button type="button" class="secondary reset-action" onclick={onRebuild}>Reset Processing Cache</button>
            </div>
          </details>
        </header>

        <p class="description">Extracts searchable text from {selectedPlugin.handles.join(', ')} files.</p>

        <section class="panel">
          <h4>Status</h4>
          <p class="summary-line">
            <strong>{selectedSummary?.indexed_count ?? 0} processed</strong>
            <span>·</span>
            <strong>{selectedSummary?.attention_count ?? 0} need attention</strong>
          </p>
          <p class="muted">{indexingLabel}</p>
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
          <button type="button" class="linkish" onclick={onOpenFolder}>Open plugin folder</button>
        </section>

        <section class="panel issues-panel">
          <div class="panel-header">
            <div>
              <h4>Issues</h4>
              <p class="muted">{selectedSummary?.attention_count ?? 0} files need attention</p>
            </div>
          </div>

          {#if issueCategories.length}
            <div class="issue-categories">
              {#each issueCategories as category}
                <button
                  type="button"
                  class:selected={selectedIssueCode === category.code}
                  onclick={() => (selectedIssueCode = selectedIssueCode === category.code ? null : category.code)}
                >
                  <span>{category.label}</span>
                  <strong>{category.count}</strong>
                </button>
              {/each}
            </div>

            <div class="issues-list">
              {#each selectedIssues as issue}
                <article class="issue-card">
                  <div class="issue-copy">
                    <strong>{issue.file_name}</strong>
                    <p>{labelForIssue(issue)}</p>
                    {#if retryMessage(issue.retry_after)}
                      <p class="muted">{retryMessage(issue.retry_after)}</p>
                    {/if}
                  </div>
                  <div class="issue-actions">
                    <button type="button" class="secondary" onclick={() => onRetryFailure?.(issue.source_path)}>
                      Retry
                    </button>
                    <button type="button" class="secondary" onclick={() => onRevealFailure?.(issue.source_path)}>
                      Reveal
                    </button>
                    <details class="details">
                      <summary>Details</summary>
                      <div class="details-copy">
                        <p><strong>Full path</strong><br />{issue.source_path}</p>
                        <p><strong>Error code</strong><br />{issue.error_code}</p>
                        <p><strong>Attempts</strong><br />{issue.attempts}</p>
                        <p><strong>Raw plugin output</strong><br />{issue.details}</p>
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
      {:else}
        <div class="empty-state">
          <h3>No Plugins Installed</h3>
          <p>Open the plugin folder to install plugin packages.</p>
          <button type="button" class="secondary" onclick={onOpenFolder}>Open Plugin Folder</button>
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
    grid-template-columns: 220px minmax(540px, 1fr);
    width: min(980px, calc(100vw - 36px));
    height: min(680px, calc(100vh - 48px));
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
    gap: 10px;
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

  .sidebar h2 {
    padding: 0 10px;
    color: #1c232b;
    font-size: 18px;
    font-weight: 760;
  }

  .plugin-list {
    display: grid;
    gap: 4px;
  }

  .plugin-list button,
  .issue-categories button,
  .menu-panel button {
    font: inherit;
  }

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
    text-align: left;
  }

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

  .plugin-list span:last-child.enabled {
    color: #16834a;
  }

  .detail {
    display: grid;
    align-content: start;
    gap: 18px;
    min-height: 0;
    padding: 24px 28px;
    overflow: auto;
  }

  .detail-header,
  .panel-header,
  .issue-card {
    display: flex;
    align-items: start;
    justify-content: space-between;
    gap: 16px;
  }

  .detail-header p,
  .description,
  .summary-line,
  .muted,
  .issue-copy p {
    margin: 0;
  }

  .detail-header h3 {
    color: #1c232b;
    font-size: 22px;
    font-weight: 780;
  }

  .description {
    color: #45515d;
    line-height: 1.5;
  }

  .panel {
    display: grid;
    gap: 10px;
    padding: 18px;
    border: 1px solid #e2e7e2;
    border-radius: 12px;
    background: #fbfcfb;
  }

  .summary-line {
    display: flex;
    gap: 10px;
    color: #1c232b;
    font-size: 18px;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .chips span,
  .issue-categories button {
    border: 1px solid #d8dfd8;
    border-radius: 999px;
    background: #fff;
  }

  .chips span {
    padding: 7px 11px;
    color: #31404d;
    font-size: 13px;
  }

  .linkish,
  .secondary {
    border-radius: 10px;
    font: inherit;
  }

  .linkish {
    width: fit-content;
    border: 0;
    padding: 0;
    color: #0f6b3b;
    background: transparent;
  }

  .secondary {
    min-height: 36px;
    padding: 0 14px;
  }

  .secondary {
    border: 1px solid #d8dfd8;
    color: #24313d;
    background: #fff;
  }

  .reset-action {
    color: #6c5252;
    border-color: #e5d9d9;
    background: #fcf8f8;
  }

  .issues-panel {
    gap: 14px;
  }

  .issue-categories {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .issue-categories button {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 8px 12px;
    color: #24313d;
  }

  .issue-categories button.selected {
    border-color: #0f6b3b;
    color: #0f6b3b;
    background: #edf7f1;
  }

  .issues-list {
    display: grid;
    gap: 12px;
  }

  .issue-card {
    padding: 14px;
    border: 1px solid #e2e7e2;
    border-radius: 12px;
    background: #fff;
  }

  .issue-copy {
    display: grid;
    gap: 6px;
  }

  .issue-actions {
    display: flex;
    align-items: start;
    gap: 8px;
    flex-wrap: wrap;
    justify-content: end;
  }

  .details summary,
  .menu summary {
    cursor: pointer;
    list-style: none;
  }

  .details-copy {
    margin-top: 10px;
    color: #45515d;
    font-size: 13px;
    line-height: 1.5;
  }

  .menu {
    position: relative;
  }

  .menu summary {
    min-height: 36px;
    border: 1px solid #d8dfd8;
    border-radius: 10px;
    padding: 8px 12px;
    color: #24313d;
    background: #fff;
  }

  .menu-panel {
    position: absolute;
    right: 0;
    top: calc(100% + 8px);
    z-index: 4;
    display: grid;
    gap: 8px;
    min-width: 220px;
    padding: 10px;
    border: 1px solid #d8dfd8;
    border-radius: 12px;
    background: #fff;
    box-shadow: 0 12px 24px rgba(27, 35, 42, 0.14);
  }

  .empty-state {
    display: grid;
    align-content: center;
    justify-items: start;
    gap: 12px;
    min-height: 320px;
  }

  @media (max-width: 860px) {
    .plugins-dialog {
      grid-template-columns: 1fr;
      width: min(720px, calc(100vw - 24px));
      height: min(720px, calc(100vh - 24px));
      max-height: calc(100vh - 24px);
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
  }
</style>
