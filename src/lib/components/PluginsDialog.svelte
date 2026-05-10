<script lang="ts">
  import type { InstalledPluginInfo, PluginIndexStatus } from '$lib/types';

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

  let section = $state<'installed' | 'available' | 'updates'>('installed');
  let internalSelectedPluginId = $state<string | null>(null);

  $effect(() => {
    if (selectedPluginId) {
      internalSelectedPluginId = selectedPluginId;
      section = 'installed';
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
  const indexingLabel = $derived.by(() => {
    if (!status) return 'Idle';
    if (status.paused) return 'Paused';
    return status.indexing_state === 'running' ? 'Running' : status.indexing_state === 'queued' ? 'Queued' : 'Idle';
  });

  function selectPlugin(plugin: InstalledPluginInfo) {
    internalSelectedPluginId = plugin.id;
    section = 'installed';
  }
</script>

<div class="modal-layer" role="presentation">
  <button class="modal-backdrop" type="button" aria-label="Close plugin manager" onclick={onClose}></button>

  <div class="plugins-dialog" role="dialog" aria-modal="true" aria-labelledby="plugins-title">
    <aside class="sidebar">
      <h2 id="plugins-title">Plugins</h2>
      <button type="button" class:active={section === 'installed'} onclick={() => (section = 'installed')}>
        Installed
      </button>
      <button type="button" class:active={section === 'available'} onclick={() => (section = 'available')}>
        Available
      </button>
      <button type="button" class:active={section === 'updates'} onclick={() => (section = 'updates')}>
        Updates
      </button>

      {#if section === 'installed' && installedPlugins.length}
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
      {#if section !== 'installed'}
        <div class="empty-state">
          <h3>{section === 'available' ? 'Available Plugins' : 'Plugin Updates'}</h3>
          <p>{section === 'available' ? 'Install plugins by dropping signed packages into the plugin folder.' : 'No plugin updates are available.'}</p>
          {#if section === 'available'}
            <button type="button" class="secondary" onclick={onOpenFolder}>Open Plugin Folder</button>
          {/if}
        </div>
      {:else if selectedPlugin}
        <header class="detail-header">
          <div>
            <h3>{selectedPlugin.name}</h3>
            <p>Version {selectedPlugin.version}</p>
          </div>
          <span class:enabled={selectedPlugin.enabled} class="status-pill">
            {selectedPlugin.enabled ? 'Enabled' : 'Disabled'}
          </span>
        </header>

        <p class="description">
          Extracts searchable text and page metadata from {selectedPlugin.handles.join(', ')} files.
        </p>

        <dl class="detail-grid">
          <div>
            <dt>Capabilities</dt>
            <dd>
              <span>✓ Text extraction</span>
              <span>{selectedPlugin.capabilities.layout ? '✓' : '✗'} Layout preservation</span>
              <span>{selectedPlugin.capabilities.ocr ? '✓' : '✗'} OCR</span>
            </dd>
          </div>
          <div>
            <dt>Handles</dt>
            <dd>{selectedPlugin.handles.join(', ')}</dd>
          </div>
          <div>
            <dt>Storage</dt>
            <dd>{selectedPlugin.root_path}</dd>
          </div>
          <div>
            <dt>Status</dt>
            <dd>{indexingLabel}</dd>
          </div>
        </dl>

        {#if status}
          <div class="index-summary">
            <strong>Indexing</strong>
            <span>{status.total_known} total</span>
            <span>{status.ready_count} ready</span>
            <span>{status.processing_count} processing</span>
            <span>{status.queued_count} queued now</span>
            <span>{status.pending_count} pending</span>
            {#if status.failed_count > 0}
              <span>{status.failed_count} failed</span>
            {/if}
          </div>
        {/if}

        {#if status?.failures.length}
          <div class="failures">
            <div class="failures-header">
              <strong>Failures</strong>
              <span>{status.failed_count} files failed indexing</span>
            </div>
            <div class="failures-list">
              {#each status.failures as failure}
                <article class="failure-item">
                  <div class="failure-main">
                    <strong>{failure.source_path.split('/').at(-1)}</strong>
                    <p class="failure-path">{failure.source_path}</p>
                    <p class="failure-message">{failure.message}</p>
                    {#if failure.next_retry_at}
                      <p class="failure-retry">Retry after {failure.next_retry_at}</p>
                    {/if}
                  </div>
                  <div class="failure-actions">
                    <button type="button" class="secondary" onclick={() => onRetryFailure?.(failure.source_path)}>
                      Retry
                    </button>
                    <button type="button" class="secondary" onclick={() => onRevealFailure?.(failure.source_path)}>
                      Reveal
                    </button>
                    <details>
                      <summary>Details</summary>
                      <pre>{failure.details}</pre>
                    </details>
                  </div>
                </article>
              {/each}
            </div>
          </div>
        {/if}

        <div class="actions">
          <button type="button" class="secondary" disabled>{selectedPlugin.enabled ? 'Disable' : 'Enable'}</button>
          <button type="button" class="secondary" disabled>Uninstall</button>
          <button type="button" class="secondary" onclick={onOpenFolder}>Open Folder</button>
          <button type="button" class="secondary" onclick={onTogglePaused}>
            {status?.paused ? 'Resume Background Indexing' : 'Pause Background Indexing'}
          </button>
          <button type="button" class="primary" onclick={onRebuild}>Rebuild Plugin Cache</button>
          <button type="button" class="secondary" onclick={onRefresh}>Refresh</button>
        </div>
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
    grid-template-columns: 220px minmax(480px, 1fr);
    width: min(920px, calc(100vw - 36px));
    min-height: min(620px, calc(100vh - 48px));
    border: 1px solid #d9e0d9;
    border-radius: 14px;
    background: #ffffff;
    box-shadow: 0 22px 44px rgba(27, 35, 42, 0.16);
    overflow: hidden;
  }

  .sidebar {
    display: grid;
    grid-auto-rows: min-content;
    gap: 6px;
    padding: 20px 14px;
    border-right: 1px solid #e2e7e2;
    background: #f6f8f6;
  }

  .sidebar h2 {
    margin: 0 0 10px;
    padding: 0 10px;
    color: #1c232b;
    font-size: 18px;
    font-weight: 760;
  }

  .sidebar > button,
  .plugin-list button,
  .actions button {
    font: inherit;
  }

  .sidebar > button,
  .plugin-list button {
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 34px;
    border: 0;
    border-radius: 8px;
    padding: 0 10px;
    color: #2e3842;
    background: transparent;
    text-align: left;
  }

  .sidebar > button.active,
  .plugin-list button.selected {
    background: #e7efe7;
    color: #0f6b3b;
    font-weight: 700;
  }

  .plugin-list {
    display: grid;
    gap: 4px;
    margin-top: 10px;
  }

  .plugin-list span:last-child,
  .status-pill {
    color: #65707a;
    font-size: 12px;
    font-weight: 700;
  }

  .plugin-list span:last-child.enabled,
  .status-pill.enabled {
    color: #16834a;
  }

  .detail {
    display: grid;
    align-content: start;
    gap: 18px;
    padding: 24px 28px;
  }

  .detail-header {
    display: flex;
    align-items: start;
    justify-content: space-between;
    gap: 16px;
  }

  .detail-header h3 {
    margin: 0;
    color: #1c232b;
    font-size: 22px;
    font-weight: 780;
  }

  .detail-header p,
  .description,
  .empty-state p,
  .failures p {
    margin: 0;
    color: #5e6974;
    font-size: 14px;
    line-height: 1.5;
  }

  .detail-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 16px 24px;
    margin: 0;
  }

  .detail-grid div {
    display: grid;
    gap: 6px;
  }

  dt {
    color: #77828c;
    font-size: 12px;
    font-weight: 800;
    text-transform: uppercase;
  }

  dd {
    display: grid;
    gap: 4px;
    margin: 0;
    color: #1f2831;
    font-size: 14px;
    font-weight: 600;
  }

  .index-summary {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    border-top: 1px solid #e5e9e5;
    border-bottom: 1px solid #e5e9e5;
    padding: 12px 0;
    color: #50606c;
    font-size: 13px;
    font-weight: 700;
  }

  .failures {
    display: grid;
    gap: 8px;
  }

  .failures-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .failures strong {
    color: #1c232b;
    font-size: 14px;
  }

  .failures-header span,
  .failure-path,
  .failure-retry {
    color: #6a7680;
    font-size: 12px;
    font-weight: 600;
  }

  .failures-list {
    display: grid;
    gap: 10px;
    max-height: 240px;
    overflow: auto;
    padding-right: 6px;
  }

  .failure-item {
    display: grid;
    gap: 10px;
    border: 1px solid #e4e8e4;
    border-radius: 10px;
    padding: 12px;
    background: #fbfcfb;
  }

  .failure-main {
    display: grid;
    gap: 4px;
  }

  .failure-main strong {
    font-size: 13px;
  }

  .failure-message {
    margin: 0;
    color: #1f2831;
    font-size: 13px;
    font-weight: 700;
  }

  .failure-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: start;
  }

  details {
    min-width: 180px;
  }

  summary {
    color: #2f3942;
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
    user-select: none;
  }

  pre {
    margin: 8px 0 0;
    white-space: pre-wrap;
    word-break: break-word;
    color: #5b6670;
    font-size: 12px;
    line-height: 1.45;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    margin-top: auto;
  }

  .actions button,
  .empty-state button {
    min-height: 36px;
    border-radius: 9px;
    padding: 0 14px;
  }

  .primary {
    border: 1px solid #16834a;
    color: #ffffff;
    background: #16834a;
  }

  .secondary {
    border: 1px solid #d6ddd6;
    color: #2e3842;
    background: #ffffff;
  }

  .empty-state {
    display: grid;
    align-content: center;
    gap: 12px;
    min-height: 360px;
  }

  @media (max-width: 920px) {
    .plugins-dialog {
      grid-template-columns: 1fr;
      width: min(760px, calc(100vw - 24px));
    }

    .sidebar {
      border-right: 0;
      border-bottom: 1px solid #e2e7e2;
    }

    .detail-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
