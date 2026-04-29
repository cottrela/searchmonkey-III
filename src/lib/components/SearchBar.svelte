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
    onSearch,
    onStop
  }: {
    query: string;
    options: SearchOptions;
    searching?: boolean;
    stopping?: boolean;
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
      placeholder="Search files (regex supported)..."
      autocomplete="off"
      spellcheck="false"
    />
  </div>

  <div class="actions">
    <button class="primary" type="submit" disabled={stopping}>
      {searching || stopping ? 'Stop' : 'Search'}
    </button>
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
      <span>Case sensitive</span>
    </label>
    <label class="toggle">
      <input type="checkbox" bind:checked={options.hidden} />
      <span>Include hidden files</span>
    </label>
  </div>
</form>

<style>
  .search-bar {
    display: grid;
    grid-template-columns: minmax(320px, 1fr) auto;
    gap: 14px 18px;
    align-items: end;
    padding: 18px 20px;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .query-wrap {
    display: grid;
    gap: 7px;
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
    height: 52px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    padding: 0 14px;
    color: var(--text);
    background: var(--input);
    font: inherit;
    font-size: 18px;
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
    height: 52px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    padding: 0 14px;
    font: inherit;
    font-weight: 700;
  }

  .primary {
    width: 92px;
    flex: 0 0 92px;
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

  .search-status {
    display: inline-flex;
    align-items: center;
    height: 52px;
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
    height: 26px;
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

  @media (max-width: 760px) {
    .search-bar {
      grid-template-columns: minmax(0, 1fr);
    }

    .actions {
      justify-content: flex-start;
    }
  }

  @media (max-width: 520px) {
    .query-meta {
      align-items: flex-start;
      flex-direction: column;
      gap: 4px;
    }

    .actions {
      display: grid;
      grid-template-columns: minmax(0, 1fr);
    }

    .primary {
      width: 100%;
      flex-basis: auto;
    }

    .search-status {
      height: auto;
      min-height: 20px;
    }
  }
</style>
