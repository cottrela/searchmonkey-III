<script lang="ts">
  import { defaultSearchOptions, type SearchCriteria, type SearchOptions } from '$lib/types';

  type LayoutMode = 'focus' | 'split' | 'full';

  let {
    query = $bindable(''),
    options = $bindable<SearchOptions>(defaultSearchOptions()),
    searching = false,
    savedSearches = [],
    layoutMode = 'split',
    availableLayoutModes = ['focus', 'split', 'full'],
    onFilters,
    onLayoutMode,
    onRegexTester,
    onApplyCriteria,
    onSaveRequest,
    onRenameCriteria,
    onDeleteCriteria,
    onSearch,
    onCancel
  }: {
    query: string;
    options: SearchOptions;
    searching?: boolean;
    savedSearches?: SearchCriteria[];
    layoutMode?: LayoutMode;
    availableLayoutModes?: LayoutMode[];
    onFilters?: () => void;
    onLayoutMode?: (mode: LayoutMode) => void;
    onRegexTester?: () => void;
    onApplyCriteria?: (criteria: SearchCriteria) => void;
    onSaveRequest?: () => void;
    onRenameCriteria?: (criteria: SearchCriteria) => void;
    onDeleteCriteria?: (criteria: SearchCriteria) => void;
    onSearch: () => void;
    onCancel?: () => void;
  } = $props();

  let savedMenuElement = $state<HTMLDetailsElement>();
  let savedPopoverStyle = $state('');
  const visibleLayoutModes = $derived(availableLayoutModes);

  function submit(event: SubmitEvent) {
    event.preventDefault();
    if (searching) {
      onCancel?.();
      return;
    }

    onSearch();
  }

  function applySavedCriteria(criteria: SearchCriteria) {
    onApplyCriteria?.(criteria);
    if (savedMenuElement) {
      savedMenuElement.open = false;
    }
  }

  function positionSavedPopover() {
    if (!savedMenuElement?.open) return;

    const rect = savedMenuElement.getBoundingClientRect();
    const width = Math.min(320, window.innerWidth - 16);
    const left = Math.min(Math.max(8, rect.right - width), window.innerWidth - width - 8);
    const top = Math.min(rect.bottom + 4, window.innerHeight - 8);

    savedPopoverStyle = `--saved-popover-left: ${left}px; --saved-popover-top: ${top}px; --saved-popover-width: ${width}px;`;
  }
</script>

<form class="search-bar" onsubmit={submit}>
  <div class="brand-anchor" aria-label="Searchmonkey III">
    <span class="brand-mark" aria-hidden="true">SM</span>
    <span class="brand-name">Searchmonkey III</span>
  </div>

  <div class="query-wrap">
    <div class="query-meta">
      <label for="search-query">Search text</label>
      <span>Enter Search</span>
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
    <div class="search-actions">
      <button class="primary" type="submit">
        {searching ? 'Stop' : 'Search'}
      </button>
      <details class="saved-menu" bind:this={savedMenuElement} ontoggle={positionSavedPopover}>
        <summary>Saved <span aria-hidden="true">▾</span></summary>
        <div class="saved-popover" style={savedPopoverStyle}>
          <button class="save-current" type="button" onclick={() => onSaveRequest?.()}>Save current search</button>
          {#if savedSearches.length}
            <div class="saved-list" aria-label="Saved searches">
              {#each savedSearches as search (search.id)}
                <div class="saved-row">
                  <button class="saved-load" type="button" title={search.name} onclick={() => applySavedCriteria(search)}>
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
        <button class="secondary filters-action" type="button" onclick={onFilters}>Filters</button>
      {/if}
    </div>

    {#if onLayoutMode && visibleLayoutModes.length > 1}
      <div class="layout-switcher" aria-label="Layout mode">
        {#if visibleLayoutModes.includes('focus')}
          <button
            type="button"
            class="mode-focus"
            class:active={layoutMode === 'focus'}
            title="Focus: results only (Ctrl/Cmd+1)"
            onclick={() => onLayoutMode?.('focus')}
          >
            Results
          </button>
        {/if}
        {#if visibleLayoutModes.includes('split')}
          <button
            type="button"
            class="mode-split"
            class:active={layoutMode === 'split'}
            title="Split: results and preview (Ctrl/Cmd+2)"
            onclick={() => onLayoutMode?.('split')}
          >
            Split
          </button>
        {/if}
        {#if visibleLayoutModes.includes('full')}
          <button
            type="button"
            class="mode-full"
            class:active={layoutMode === 'full'}
            title="Full: scope, results, and preview (Ctrl/Cmd+3)"
            onclick={() => onLayoutMode?.('full')}
          >
            Full
          </button>
        {/if}
      </div>
    {/if}
  </div>

</form>

<style>
  .search-bar {
    display: grid;
    grid-template-columns: auto minmax(320px, 1fr) auto;
    gap: 6px 16px;
    align-items: end;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .brand-anchor {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-height: 38px;
    padding-bottom: 1px;
    white-space: nowrap;
  }

  .brand-mark {
    position: relative;
    display: inline-grid;
    width: 28px;
    height: 28px;
    border: 1px solid var(--accent);
    border-radius: 999px;
    place-items: center;
    color: var(--accent);
    background: var(--accent-wash);
    font-size: 9px;
    font-weight: 900;
    letter-spacing: 0;
  }

  .brand-mark::after {
    content: "";
    position: absolute;
    right: -4px;
    bottom: 1px;
    width: 8px;
    height: 2px;
    border-radius: 99px;
    background: var(--accent);
    transform: rotate(42deg);
    transform-origin: left center;
  }

  .brand-name {
    color: var(--text);
    font-size: 14px;
    font-weight: 750;
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
    font-weight: 500;
  }

  .query-meta span {
    color: var(--muted);
    font-size: 12px;
    font-weight: 600;
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
    display: grid;
    grid-template-columns: auto auto;
    gap: 6px 12px;
    align-items: end;
    min-width: 0;
  }

  .search-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    justify-content: flex-end;
    min-width: 0;
  }

  button {
    height: 38px;
    border: 1px solid var(--border);
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
    box-shadow: 0 1px 0 rgba(11, 95, 50, 0.24);
  }

  .primary:hover,
  .primary:focus-visible {
    background: var(--accent-strong);
    outline: none;
  }

  .primary:disabled {
    border-color: var(--border-strong);
    color: var(--muted);
    background: var(--disabled);
  }

  .secondary {
    border-color: transparent;
    color: #828c96;
    background: transparent;
    font-weight: 600;
  }

  .secondary:hover,
  .secondary:focus-visible,
  .saved-menu > summary:hover,
  .saved-menu > summary:focus-visible {
    border-color: var(--border-subtle);
    color: var(--text);
    background: var(--input);
    outline: none;
  }

  .layout-switcher {
    display: inline-flex;
    height: 38px;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
    background: var(--surface);
  }

  .layout-switcher button {
    height: 100%;
    border: 0;
    border-radius: 0;
    padding: 0 9px;
    color: var(--muted);
    background: transparent;
    font-size: 12px;
  }

  .layout-switcher button + button {
    border-left: 1px solid var(--border-subtle);
  }

  .layout-switcher button.active {
    color: var(--accent-strong);
    background: var(--accent-wash);
    box-shadow: inset 0 -2px 0 var(--accent);
  }

  .saved-menu {
    position: relative;
  }

  .saved-menu > summary {
    display: inline-grid;
    grid-auto-flow: column;
    gap: 6px;
    height: 38px;
    border: 1px solid transparent;
    border-radius: 6px;
    padding: 0 12px;
    place-items: center;
    color: rgba(102, 113, 125, 0.78);
    background: transparent;
    font-weight: 550;
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
    position: fixed;
    top: var(--saved-popover-top, 42px);
    right: auto;
    left: var(--saved-popover-left, 8px);
    width: var(--saved-popover-width, 240px);
    max-width: calc(100vw - 16px);
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
    display: inline-block;
  }

  @media (max-width: 760px) {
    .search-bar {
      grid-template-columns: minmax(0, 1fr);
      gap: 8px 12px;
    }

    .brand-anchor {
      grid-row: 1;
    }

    .actions {
      grid-template-columns: minmax(0, 1fr) auto;
      align-items: start;
    }

    .search-actions {
      justify-content: flex-start;
    }

    .query-meta {
      display: none;
    }
  }

  @media (max-width: 1099px) {
    .layout-switcher .mode-full {
      display: none;
    }
  }

  @media (max-width: 849px) {
    .layout-switcher .mode-split {
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

    .brand-anchor {
      min-height: 30px;
    }

    .brand-mark {
      width: 24px;
      height: 24px;
      font-size: 8px;
    }

    .brand-name {
      font-size: 13px;
    }

    .actions {
      gap: 6px;
    }

    .search-actions {
      gap: 6px;
    }

    button,
    .saved-menu > summary,
    .layout-switcher {
      height: 32px;
    }

    button,
    .saved-menu > summary {
      padding: 0 10px;
      font-size: 12px;
      line-height: 30px;
    }

    .layout-switcher button {
      padding: 0 8px;
      font-size: 11px;
      line-height: normal;
    }

    .saved-popover {
      font-size: 12px;
    }

    .primary {
      width: 70px;
      flex-basis: 70px;
    }

  }
</style>
