<script lang="ts">
  import type { FileResultGroup, SearchMatch, SearchState } from '$lib/types';

  let {
    groups,
    query,
    regex,
    selected,
    state,
    hasSearched,
    onSelect
  }: {
    groups: FileResultGroup[];
    query: string;
    regex: boolean;
    selected: SearchMatch | null;
    state: SearchState;
    hasSearched: boolean;
    onSelect: (match: SearchMatch) => void;
  } = $props();

  function sameMatch(a: SearchMatch | null, b: SearchMatch) {
    return Boolean(a && a.path === b.path && a.line_number === b.line_number && a.line_text === b.line_text);
  }

  function snippetParts(text: string, term: string) {
    if (regex || !term) return [{ text, hit: false }];

    const lowerText = text.toLowerCase();
    const lowerTerm = term.toLowerCase();
    const parts: Array<{ text: string; hit: boolean }> = [];
    let cursor = 0;
    let index = lowerText.indexOf(lowerTerm);

    while (index !== -1) {
      if (index > cursor) {
        parts.push({ text: text.slice(cursor, index), hit: false });
      }

      parts.push({ text: text.slice(index, index + term.length), hit: true });
      cursor = index + term.length;
      index = lowerText.indexOf(lowerTerm, cursor);
    }

    if (cursor < text.length) {
      parts.push({ text: text.slice(cursor), hit: false });
    }

    return parts.length ? parts : [{ text, hit: false }];
  }
</script>

<section class="results-panel" aria-label="Search results">
  <div class="panel-title">
    <h2>Results</h2>
    {#if groups.length}
      <span>{groups.length} files</span>
    {/if}
  </div>

  {#if !hasSearched}
    <div class="empty">Choose a folder and search text files</div>
  {:else if state === 'searching' && groups.length === 0}
    <div class="empty active-search">
      <span class="spinner" aria-hidden="true"></span>
      <span>Searching current files...</span>
    </div>
  {:else if state === 'stopping' && groups.length === 0}
    <div class="empty active-search">
      <span class="spinner" aria-hidden="true"></span>
      <span>Stopping search...</span>
    </div>
  {:else if groups.length === 0}
    <div class="empty">No matches found</div>
  {:else}
    <div class="groups">
      {#each groups as group (group.path)}
        <details class="file-group" open>
          <summary>
            <span class="file-path" title={group.path}>{group.path}</span>
            <span class="count">({group.matches.length})</span>
          </summary>

          <div class="matches">
            {#each group.matches as match, index (`${match.path}:${match.line_number}:${index}`)}
              <button
                type="button"
                class:selected={sameMatch(selected, match)}
                class="match-row"
                onclick={() => onSelect(match)}
              >
                <span class="line">{match.line_number}</span>
                <span class="snippet">
                  {#each snippetParts(match.line_text, query) as part}
                    {#if part.hit}
                      <mark>{part.text}</mark>
                    {:else}
                      <span>{part.text}</span>
                    {/if}
                  {/each}
                </span>
              </button>
            {/each}
          </div>
        </details>
      {/each}
    </div>
  {/if}
</section>

<style>
  .results-panel {
    min-width: 0;
    background: var(--surface);
    overflow: auto;
  }

  .panel-title {
    position: sticky;
    top: 0;
    z-index: 1;
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 47px;
    border-bottom: 1px solid var(--border);
    padding: 0 14px;
    background: var(--surface);
  }

  h2 {
    margin: 0;
    font-size: 14px;
  }

  .panel-title span {
    color: var(--muted);
    font-size: 12px;
    font-weight: 700;
  }

  .empty {
    display: grid;
    gap: 10px;
    min-height: 220px;
    place-items: center;
    padding: 24px;
    color: var(--muted);
    text-align: center;
  }

  .active-search {
    animation: pulse-text 1.4s ease-in-out infinite;
  }

  .spinner {
    width: 18px;
    height: 18px;
    border: 2px solid var(--border-strong);
    border-top-color: var(--accent);
    border-radius: 999px;
    animation: spin 0.8s linear infinite;
  }

  .groups {
    padding: 10px;
  }

  .file-group {
    border: 1px solid var(--border-subtle);
    border-radius: 5px;
    margin-bottom: 10px;
    background: var(--panel);
  }

  summary {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 10px;
    align-items: center;
    border-bottom: 1px solid var(--border-subtle);
    padding: 9px 11px;
    cursor: pointer;
    user-select: none;
  }

  .file-path {
    min-width: 0;
    overflow: hidden;
    color: var(--text);
    font-size: 13px;
    font-weight: 700;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .count {
    color: var(--muted);
    font-size: 12px;
    font-weight: 800;
    text-align: center;
  }

  .matches {
    padding: 0;
  }

  .match-row {
    display: grid;
    grid-template-columns: 54px minmax(0, 1fr);
    gap: 8px;
    width: 100%;
    border: 0;
    border-bottom: 1px solid var(--border-subtle);
    border-radius: 0;
    padding: 7px 9px;
    color: var(--text);
    background: transparent;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .match-row:last-child {
    border-bottom: 0;
  }

  .match-row:hover,
  .match-row.selected {
    background: var(--selection);
  }

  .line {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }

  .snippet {
    min-width: 0;
    overflow: hidden;
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: 12px;
    line-height: 19px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  mark {
    border-radius: 3px;
    padding: 0 1px;
    color: #241800;
    background: #ffd86b;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @keyframes pulse-text {
    50% {
      color: var(--text);
    }
  }
</style>
