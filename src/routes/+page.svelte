<script lang="ts">
  import PreviewPanel from '$lib/components/PreviewPanel.svelte';
  import ResultsPanel from '$lib/components/ResultsPanel.svelte';
  import ScopePanel from '$lib/components/ScopePanel.svelte';
  import SearchBar from '$lib/components/SearchBar.svelte';
  import StatusBar from '$lib/components/StatusBar.svelte';
  import { startSearch as startSearchCommand, stopSearch } from '$lib/search';
  import type {
    FileResultGroup,
    SearchMatch,
    SearchOptions,
    SearchState,
    SearchStreamEvent
  } from '$lib/types';

  let query = $state('');
  let path = $state('');
  let includePatterns = $state('');
  let excludePatterns = $state('');
  let contextLines = $state(0);
  let options = $state<SearchOptions>({
    regex: false,
    case_sensitive: false,
    hidden: false
  });

  let matches = $state<SearchMatch[]>([]);
  let selected = $state<SearchMatch | null>(null);
  let searchState = $state<SearchState>('idle');
  let errorMessage = $state('');
  let hasSearched = $state(false);
  let activeSearchId = $state<number | null>(null);
  let nextSearchId = 1;

  const groups = $derived.by(() => groupMatches(matches));
  const selectedIndex = $derived.by(() => {
    if (!selected) return -1;
    const current = selected;
    return matches.findIndex((match) => sameMatch(match, current));
  });

  function groupMatches(searchMatches: SearchMatch[]): FileResultGroup[] {
    const byPath = new Map<string, SearchMatch[]>();

    for (const match of searchMatches) {
      const fileMatches = byPath.get(match.path);
      if (fileMatches) {
        fileMatches.push(match);
      } else {
        byPath.set(match.path, [match]);
      }
    }

    return Array.from(byPath, ([filePath, fileMatches]) => ({
      path: filePath,
      matches: fileMatches
    }));
  }

  function sameMatch(a: SearchMatch, b: SearchMatch) {
    return a.path === b.path && a.line_number === b.line_number && a.line_text === b.line_text;
  }

  function normalizeError(error: unknown) {
    if (typeof error === 'string') return error;
    if (error instanceof Error) return error.message;
    return 'Search failed. Check the folder path and search options.';
  }

  function handleSearchEvent(event: SearchStreamEvent) {
    if (event.search_id !== activeSearchId) return;

    switch (event.type) {
      case 'started':
        searchState = 'searching';
        errorMessage = '';
        break;
      case 'match':
        if (searchState === 'stopping') return;
        matches = [...matches, event.result];
        selected = selected ?? event.result;
        break;
      case 'error':
        errorMessage = event.message;
        break;
      case 'finished':
        searchState = errorMessage && matches.length === 0 ? 'error' : 'done';
        activeSearchId = null;
        break;
      case 'cancelled':
        searchState = 'done';
        errorMessage = `Search stopped after ${matches.length} matches.`;
        activeSearchId = null;
        break;
    }
  }

  async function startSearch() {
    const cleanQuery = query.trim();
    const cleanPath = path.trim();

    if (!cleanQuery) {
      searchState = 'error';
      errorMessage = 'Enter search text before starting.';
      return;
    }

    if (!cleanPath) {
      searchState = 'error';
      errorMessage = 'Choose a folder or file path before starting.';
      return;
    }

    searchState = 'searching';
    errorMessage = '';
    hasSearched = true;
    selected = null;
    matches = [];
    const searchId = nextSearchId;
    nextSearchId += 1;
    activeSearchId = searchId;

    try {
      await startSearchCommand(
        {
          query: cleanQuery,
          path: cleanPath,
          regex: options.regex,
          case_sensitive: options.case_sensitive,
          hidden: options.hidden
        },
        searchId,
        handleSearchEvent
      );
    } catch (error) {
      matches = [];
      selected = null;
      activeSearchId = null;
      searchState = 'error';
      errorMessage = normalizeError(error);
    }
  }

  async function stopCurrentSearch() {
    if (searchState !== 'searching' || activeSearchId === null) return;

    try {
      searchState = 'stopping';
      errorMessage = 'Stopping search...';
      await stopSearch(activeSearchId);
    } catch (error) {
      searchState = 'error';
      errorMessage = normalizeError(error);
    }
  }

  function selectMatch(match: SearchMatch) {
    selected = match;
  }

  function selectOffset(offset: number) {
    if (!matches.length) return;

    const currentIndex = selectedIndex >= 0 ? selectedIndex : 0;
    const nextIndex = (currentIndex + offset + matches.length) % matches.length;
    selected = matches[nextIndex];
  }
</script>

<svelte:head>
  <title>SearchMonkey III</title>
</svelte:head>

<main class="app-shell">
  <SearchBar
    bind:query
    bind:options
    searching={searchState === 'searching' || searchState === 'stopping'}
    stopping={searchState === 'stopping'}
    onSearch={startSearch}
    onStop={stopCurrentSearch}
  />

  <div class="workspace">
    <ScopePanel bind:path bind:includePatterns bind:excludePatterns bind:contextLines />
    <ResultsPanel
      {groups}
      {query}
      regex={options.regex}
      {selected}
      state={searchState}
      {hasSearched}
      onSelect={selectMatch}
    />
    <PreviewPanel
      {selected}
      {selectedIndex}
      total={matches.length}
      {query}
      regex={options.regex}
      onPrevious={() => selectOffset(-1)}
      onNext={() => selectOffset(1)}
    />
  </div>

  <StatusBar
    state={searchState}
    totalMatches={matches.length}
    filesWithMatches={groups.length}
    {errorMessage}
  />
</main>

<style>
  :global(*) {
    box-sizing: border-box;
  }

  :global(:root) {
    font-family:
      Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    color: #1e252d;
    background: #eef1f4;
    font-synthesis: none;
    text-rendering: optimizeLegibility;
    -webkit-font-smoothing: antialiased;
    --text: #1e252d;
    --muted: #66717d;
    --surface: #f8f9fb;
    --panel: #ffffff;
    --input: #ffffff;
    --disabled: #edf0f3;
    --border: #d9dee5;
    --border-subtle: #e7ebef;
    --border-strong: #c5ccd5;
    --accent: #256d8f;
    --accent-soft: #93bfce;
    --focus: rgba(37, 109, 143, 0.18);
    --selection: #e8f3f7;
    --ok: #2f855a;
    --danger: #ba3c32;
  }

  :global(body) {
    margin: 0;
    min-width: 900px;
    min-height: 100vh;
    overflow: hidden;
    color: var(--text);
    background: #eef1f4;
  }

  :global(button),
  :global(input) {
    font-family: inherit;
  }

  .app-shell {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    width: 100vw;
    height: 100vh;
    background: var(--surface);
  }

  .workspace {
    display: grid;
    grid-template-columns: 280px minmax(360px, 1fr) 320px;
    min-height: 0;
  }
</style>
