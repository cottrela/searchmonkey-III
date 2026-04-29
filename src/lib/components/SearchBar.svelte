<script lang="ts">
  import type { SearchOptions } from '$lib/types';

  let {
    query = $bindable(''),
    options = $bindable<SearchOptions>({
      regex: false,
      case_sensitive: false,
      hidden: false
    }),
    searching = false,
    stopping = false,
    onFilters,
    onSearch,
    onStop
  }: {
    query: string;
    options: SearchOptions;
    searching?: boolean;
    stopping?: boolean;
    onFilters?: () => void;
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
    {#if onFilters}
      <button class="secondary filters-action" type="button" onclick={onFilters}>Filters &amp; Scope</button>
    {/if}
    {#if searching || stopping}
      <span class="search-status" aria-live="polite">
        {stopping ? 'Cancelling...' : 'Searching...'}
      </span>
    {/if}
  </div>

  <div class="toggles" aria-label="Search options">
    <label class="toggle">
      <input type="checkbox" bind:checked={options.regex} />
      <span>Regex</span>
    </label>
    <label class="toggle">
      <input type="checkbox" bind:checked={options.case_sensitive} />
      <span>Case</span>
    </label>
    <label class="toggle">
      <input type="checkbox" bind:checked={options.hidden} />
      <span>Hidden</span>
    </label>
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

  .toggles {
    display: flex;
    grid-column: 1 / -1;
    gap: 8px;
    flex-wrap: wrap;
  }

  .toggle {
    display: inline-flex;
    align-items: center;
    height: 24px;
    border: 1px solid var(--border-subtle);
    border-radius: 999px;
    padding: 0 11px;
    color: var(--muted);
    background: var(--panel);
    font-size: 12px;
    font-weight: 650;
    transition:
      border-color 120ms ease,
      color 120ms ease,
      background 120ms ease;
  }

  .toggle input {
    position: absolute;
    opacity: 0;
    pointer-events: none;
  }

  .toggle:has(input:checked) {
    border-color: var(--accent-soft);
    color: var(--text);
    background: var(--selection);
  }

  .toggle:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--focus);
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
    .search-status {
      height: 32px;
    }

    button {
      padding: 0 10px;
      font-size: 12px;
      line-height: 30px;
    }

    .primary {
      width: 70px;
      flex-basis: 70px;
    }

    .search-status {
      height: auto;
      min-height: 20px;
    }

    .toggles {
      display: none;
    }
  }
</style>
