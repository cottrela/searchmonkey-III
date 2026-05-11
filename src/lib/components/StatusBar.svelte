<script lang="ts">
  import type { PluginIndexStatus, SearchState } from '$lib/types';

  let {
    state,
    totalMatches,
    filesWithMatches,
    elapsedMs = 0,
    errorMessage = '',
    pluginStatus = null,
    onManagePlugins
  }: {
    state: SearchState;
    totalMatches: number;
    filesWithMatches: number;
    elapsedMs?: number;
    errorMessage?: string;
    pluginStatus?: PluginIndexStatus | null;
    onManagePlugins?: () => void;
  } = $props();

  const labels: Record<SearchState, string> = {
    idle: 'Ready',
    starting: 'Starting',
    running: 'Searching',
    cancelling: 'Cancelling',
    completed: 'Done',
    cancelled: 'Cancelled',
    failed: 'Error'
  };

  const matchLabel = $derived(`${totalMatches} ${totalMatches === 1 ? 'match' : 'matches'}`);
  const elapsedLabel = $derived(`${(elapsedMs / 1000).toFixed(2)}s`);
  const stateLabel = $derived.by(() => {
    if (state === 'starting' || state === 'running' || state === 'cancelling') {
      return elapsedLabel;
    }

    if ((state === 'completed' || state === 'cancelled') && elapsedMs > 0) {
      return elapsedLabel;
    }

    return labels[state];
  });
  const pluginSummary = $derived.by(() => {
    if (!pluginStatus) return null;
    const totals = pluginStatus.plugin_summaries.reduce(
      (acc, summary) => {
        acc.attention += summary.attention_count;
        acc.processing += summary.processing_count;
        acc.queued += summary.queued_count;
        acc.blocked += pluginStatus.issues.filter(
          (issue) => issue.plugin_id === summary.plugin_id && issue.attempts >= 4
        ).length;
        return acc;
      },
      { attention: 0, processing: 0, queued: 0, blocked: 0 }
    );
    const tone = totals.blocked > 0 ? 'blocked' : totals.attention > 0 ? 'warning' : 'none';
    return {
      label: `Plugins: ${pluginStatus.plugin_state}`,
      tone
    };
  });
</script>

<footer
  class="status-bar"
  class:active={state === 'starting' || state === 'running' || state === 'cancelling'}
  class:error={state === 'failed'}
>
  <div class="state">
    <span class="dot" aria-hidden="true"></span>
    <span class="live-label">Live</span>
    <strong>{stateLabel}</strong>
    {#if errorMessage}
      <span class="message">{errorMessage}</span>
    {/if}
  </div>

  <div class="metrics">
    <span>{matchLabel}</span>
    <span>{filesWithMatches} files</span>
    {#if pluginSummary}
      <button type="button" class="plugin-summary" onclick={onManagePlugins}>
        <span>{pluginSummary.label}</span>
        {#if pluginSummary.tone !== 'none'}
          <span
            class:warning={pluginSummary.tone === 'warning'}
            class:blocked={pluginSummary.tone === 'blocked'}
            class="plugin-triangle"
            aria-hidden="true"
          >
            ▲
          </span>
        {/if}
      </button>
    {/if}
    {#if state === 'starting' || state === 'running' || state === 'cancelling'}
      <span>Scanning current files</span>
    {/if}
    <span class="tagline">Fast grep with smarter files.</span>
  </div>
</footer>

<style>
  .status-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    min-height: 36px;
    border-top: 1px solid var(--border);
    padding: 0 14px;
    color: var(--muted);
    background: var(--surface);
    font-size: 12px;
  }

  .state,
  .metrics {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 6px;
  }

  .metrics {
    flex-wrap: wrap;
    justify-content: flex-end;
  }

  .metrics span + span {
    border-left: 1px solid var(--border);
    padding-left: 8px;
  }

  .metrics > * + * {
    border-left: 1px solid var(--border);
    padding-left: 8px;
  }

  .metrics span {
    color: #7d8790;
    font-weight: 550;
  }

  .plugin-summary {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    border: 0;
    border-left: 1px solid var(--border);
    padding: 0 0 0 8px;
    color: #7d8790;
    background: transparent;
    cursor: pointer;
    font: inherit;
    font-weight: 550;
    transition: color 120ms ease;
  }

  .plugin-summary:hover {
    color: var(--accent-strong);
  }

  .plugin-triangle {
    font-size: 10px;
    line-height: 1;
    opacity: 0.8;
  }

  .plugin-triangle.warning {
    color: #c27a15;
  }

  .plugin-triangle.blocked {
    color: #c24b3a;
  }

  .tagline {
    color: #8b949d;
    font-weight: 500;
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 999px;
    background: var(--ok);
  }

  .live-label {
    color: var(--accent-strong);
    font-size: 11px;
    font-weight: 850;
    text-transform: uppercase;
  }

  .status-bar.active:not(.error) .dot {
    animation: status-pulse 1.2s ease-in-out infinite;
  }

  .error .dot {
    background: var(--danger);
    animation: none;
  }

  strong {
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }

  .message {
    min-width: 0;
    overflow: hidden;
    color: var(--muted);
    font-weight: 650;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .error .message {
    color: var(--danger);
    font-weight: 700;
  }

  @media (max-width: 599px) {
    .status-bar {
      min-height: 32px;
      padding: 0 10px;
    }

    .metrics span:not(:first-child) {
      display: none;
    }

    .message {
      max-width: 46vw;
    }
  }

  @keyframes status-pulse {
    50% {
      opacity: 0.42;
      transform: scale(0.82);
    }
  }
</style>
