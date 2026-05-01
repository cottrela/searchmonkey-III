<script lang="ts">
  import type { SearchState } from '$lib/types';

  let {
    state,
    totalMatches,
    filesWithMatches,
    elapsedMs = 0,
    errorMessage = ''
  }: {
    state: SearchState;
    totalMatches: number;
    filesWithMatches: number;
    elapsedMs?: number;
    errorMessage?: string;
  } = $props();

  const labels: Record<SearchState, string> = {
    idle: 'Idle',
    starting: 'Starting...',
    running: 'Searching...',
    cancelling: 'Cancelling...',
    completed: 'Done',
    cancelled: 'Cancelled',
    failed: 'Error'
  };

  const matchLabel = $derived(`${totalMatches} ${totalMatches === 1 ? 'match' : 'matches'}`);
  const elapsedLabel = $derived(`${(elapsedMs / 1000).toFixed(2)}s`);
  const stateLabel = $derived.by(() => {
    if (state === 'starting' || state === 'running' || state === 'cancelling') {
      return `${labels[state]} ${elapsedLabel}`;
    }

    if ((state === 'completed' || state === 'cancelled') && elapsedMs > 0) {
      return `${labels[state]} in ${elapsedLabel}`;
    }

    return labels[state];
  });
</script>

<footer
  class="status-bar"
  class:active={state === 'starting' || state === 'running' || state === 'cancelling'}
  class:error={state === 'failed'}
>
  <div class="state">
    <span class="dot" aria-hidden="true"></span>
    <strong>{stateLabel}</strong>
    {#if errorMessage}
      <span class="message">{errorMessage}</span>
    {/if}
  </div>

  <div class="metrics">
    <span>{matchLabel}</span>
    <span>{filesWithMatches} files</span>
    {#if state === 'starting' || state === 'running' || state === 'cancelling'}
      <span>Scanning current files</span>
    {/if}
    <span>Searches current files directly. No index.</span>
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
    gap: 10px;
  }

  .metrics {
    flex-wrap: wrap;
    justify-content: flex-end;
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 999px;
    background: var(--ok);
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
