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
    onSearch
  }: {
    query: string;
    options: SearchOptions;
    searching?: boolean;
    onSearch: () => void;
  } = $props();

  function submit(event: SubmitEvent) {
    event.preventDefault();
    onSearch();
  }
</script>

<form class="search-bar" onsubmit={submit}>
  <div class="query-wrap">
    <div class="query-meta">
      <label for="search-query">Search text</label>
      <span>Press Enter to search</span>
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
    <button class="primary" type="submit" disabled={searching}>
      {searching ? 'Searching' : 'Start Search'}
    </button>
    <button class="secondary" type="button" disabled title="Cancellation is not implemented yet">
      Stop
    </button>
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
    gap: 8px;
  }

  button {
    height: 52px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    padding: 0 14px;
    font: inherit;
    font-weight: 700;
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
    color: var(--muted);
    background: var(--input);
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
    gap: 7px;
    height: 28px;
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 0 10px;
    color: var(--text);
    background: var(--panel);
    font-size: 12px;
    font-weight: 700;
  }

  .toggle input {
    accent-color: var(--accent);
  }
</style>
