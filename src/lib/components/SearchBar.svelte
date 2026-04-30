<script lang="ts">
  import { defaultSearchOptions, type SearchCriteria, type SearchOptions } from '$lib/types';

  let {
    query = $bindable(''),
    options = $bindable<SearchOptions>(defaultSearchOptions()),
    searching = false,
    stopping = false,
    savedSearches = [],
    onFilters,
    onRegexTester,
    onApplyCriteria,
    onSaveRequest,
    onRenameCriteria,
    onDeleteCriteria,
    onSearch,
    onStop
  }: {
    query: string;
    options: SearchOptions;
    searching?: boolean;
    stopping?: boolean;
    savedSearches?: SearchCriteria[];
    onFilters?: () => void;
    onRegexTester?: () => void;
    onApplyCriteria?: (criteria: SearchCriteria) => void;
    onSaveRequest?: () => void;
    onRenameCriteria?: (criteria: SearchCriteria) => void;
    onDeleteCriteria?: (criteria: SearchCriteria) => void;
    onSearch: () => void;
    onStop: () => void;
  } = $props();

  function submit(event: SubmitEvent) {
    event.preventDefault();
    if (searching || stopping) {
      onStop();
      return;
    }

    onSearch();
  }
</script>

<form class="search-bar" onsubmit={submit}>
  <div class="query-wrap">
    <div class="query-meta">
      <label for="search-query">Search text</label>
      <span>Enter Search / Esc Cancel</span>
    </div>
    <input
      id="search-query"
      class="query-input"
      bind:value={query}
      placeholder="Search files (use Regex for patterns)..."
      autocomplete="off"
      spellcheck="false"
    />
  </div>

  <div class="actions">
    <button class="primary" type="submit" disabled={stopping}>
      {searching || stopping ? 'Stop' : 'Search'}
    </button>
    <details class="saved-menu">
      <summary>Saved</summary>
      <div class="saved-popover">
        <button class="save-current" type="button" onclick={() => onSaveRequest?.()}>Save current search</button>
        {#if savedSearches.length}
          <div class="saved-list" aria-label="Saved searches">
            {#each savedSearches as search (search.id)}
              <div class="saved-row">
                <button class="saved-load" type="button" title={search.name} onclick={() => onApplyCriteria?.(search)}>
                  {search.name}
                </button>
                <details class="saved-actions">
                  <summary aria-label={`Actions for ${search.name}`}>...</summary>
                  <div class="saved-action-menu">
                    <button type="button" onclick={() => onRenameCriteria?.(search)}>Rename</button>
                    <button type="button" onclick={() => onDeleteCriteria?.(search)}>Delete</button>
                  </div>
                </details>
              </div>
            {/each}
          </div>
        {:else}
          <div class="saved-empty">No saved searches</div>
        {/if}
      </div>
    </details>
    {#if onRegexTester && options.search_mode === 'regex'}
      <button class="secondary regex-tool" type="button" title="Open regex tester (Ctrl+Shift+R / Cmd+Shift+R)" onclick={onRegexTester}>
        Regex
      </button>
    {/if}
    {#if onFilters}
      <button class="secondary filters-action" type="button" onclick={onFilters}>Filters &amp; Scope</button>
    {/if}
    {#if searching || stopping}
      <span class="search-status" aria-live="polite">
        {stopping ? 'Cancelling...' : 'Searching...'}
      </span>
    {/if}
  </div>

</form>

<style>
  .search-bar {
    display: grid;
    grid-template-columns: minmax(320px, 1fr) auto;
    gap: 8px 12px;
    align-items: end;
    padding: 10px 12px;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .query-wrap {
    display: grid;
    gap: 5px;
    min-width: 0;
  }

  .query-meta {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  label {
    color: var(--muted);
    font-size: 12px;
    font-weight: 600;
  }

  .query-meta span {
    color: var(--muted);
    font-size: 12px;
    font-weight: 700;
  }

  .query-input {
    height: 38px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    padding: 0 11px;
    color: var(--text);
    background: var(--input);
    font: inherit;
    font-size: 14px;
    font-weight: 650;
  }

  .query-input:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--focus);
    outline: none;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    justify-content: flex-end;
    min-width: 0;
  }

  button {
    height: 38px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    padding: 0 12px;
    font: inherit;
    font-weight: 700;
  }

  .primary {
    width: 82px;
    flex: 0 0 82px;
  }

  button:not(:disabled) {
    cursor: pointer;
  }

  .primary {
    border-color: var(--accent);
    color: #ffffff;
    background: var(--accent);
  }

  .primary:disabled {
    border-color: var(--border-strong);
    color: var(--muted);
    background: var(--disabled);
  }

  .secondary {
    color: var(--text);
    background: var(--input);
  }

  .saved-menu {
    position: relative;
  }

  .saved-menu > summary {
    display: inline-grid;
    height: 38px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    padding: 0 12px;
    place-items: center;
    color: var(--text);
    background: var(--input);
    font-weight: 700;
    cursor: pointer;
    list-style: none;
  }

  .saved-menu > summary::-webkit-details-marker,
  .saved-actions > summary::-webkit-details-marker {
    display: none;
  }

  .saved-popover,
  .saved-action-menu {
    position: absolute;
    z-index: 50;
    display: grid;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--panel);
    box-shadow: 0 10px 24px rgba(30, 37, 45, 0.16);
  }

  .saved-popover {
    top: 42px;
    right: 0;
    width: 240px;
    padding: 5px;
  }

  .save-current,
  .saved-load,
  .saved-action-menu button {
    height: 30px;
    border: 0;
    border-radius: 4px;
    padding: 0 8px;
    color: var(--text);
    background: transparent;
    font: inherit;
    font-size: 12px;
    font-weight: 750;
    text-align: left;
  }

  .save-current {
    border-bottom: 1px solid var(--border-subtle);
    border-radius: 4px 4px 0 0;
  }

  .saved-list {
    display: grid;
    gap: 2px;
    padding-top: 5px;
  }

  .saved-row {
    position: relative;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 30px;
    gap: 2px;
  }

  .saved-load {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .saved-actions > summary {
    display: inline-grid;
    width: 30px;
    height: 30px;
    border-radius: 4px;
    place-items: center;
    color: var(--muted);
    cursor: pointer;
    font-weight: 900;
    list-style: none;
  }

  .saved-action-menu {
    top: 30px;
    right: 0;
    min-width: 104px;
    padding: 4px;
  }

  .saved-empty {
    padding: 9px 8px 5px;
    color: var(--muted);
    font-size: 12px;
    font-weight: 700;
  }

  .save-current:hover,
  .save-current:focus-visible,
  .saved-load:hover,
  .saved-load:focus-visible,
  .saved-actions > summary:hover,
  .saved-actions > summary:focus-visible,
  .saved-action-menu button:hover,
  .saved-action-menu button:focus-visible {
    background: var(--selection);
    outline: none;
  }

  .filters-action {
    display: none;
  }

  .search-status {
    display: inline-flex;
    align-items: center;
    height: 38px;
    color: var(--muted);
    font-size: 13px;
    font-weight: 700;
    white-space: nowrap;
  }

  @media (max-width: 1199px) {
    .filters-action {
      display: inline-block;
    }
  }

  @media (max-width: 760px) {
    .search-bar {
      grid-template-columns: minmax(0, 1fr);
    }

    .actions {
      justify-content: flex-start;
    }

    .query-meta {
      display: none;
    }
  }

  @media (max-width: 520px) {
    .search-bar {
      gap: 6px;
      padding: 7px 8px;
    }

    .query-input {
      height: 32px;
      font-size: 13px;
    }

    .actions {
      display: flex;
      gap: 6px;
    }

    button,
    .saved-menu > summary,
    .search-status {
      height: 32px;
    }

    button,
    .saved-menu > summary {
      padding: 0 10px;
      font-size: 12px;
      line-height: 30px;
    }

    .saved-popover {
      font-size: 12px;
    }

    .primary {
      width: 70px;
      flex-basis: 70px;
    }

    .search-status {
      height: auto;
      min-height: 20px;
    }

  }
</style>
