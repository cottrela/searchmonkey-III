<script lang="ts">
  import { onMount } from 'svelte';
  import PreviewPanel from '$lib/components/PreviewPanel.svelte';
  import ResultsPanel from '$lib/components/ResultsPanel.svelte';
  import ScopePanel from '$lib/components/ScopePanel.svelte';
  import SearchBar from '$lib/components/SearchBar.svelte';
  import StatusBar from '$lib/components/StatusBar.svelte';
  import {
    homeDir,
    readFilePreview,
    startSearch as startSearchCommand,
    stopSearch
  } from '$lib/search';
  import type {
    FileResultGroup,
    FilePreview,
    PreviewState,
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
  let previewData = $state<FilePreview | null>(null);
  let previewError = $state('');
  let loadedPreviewKey = '';
  let previewLoadId = 0;
  let workspaceElement = $state<HTMLElement>();
  let previewWidth = $state(360);
  let isResizingPreview = $state(false);

  const groups = $derived.by(() => groupMatches(matches));
  const selectedIndex = $derived.by(() => {
    if (!selected) return -1;
    const current = selected;
    return matches.findIndex((match) => sameMatch(match, current));
  });
  const preview = $derived.by(() => {
    if (!selected) {
      return {
        filePath: '',
        filePreview: null,
        matches: [],
        activeMatchIndex: -1
      } satisfies PreviewState;
    }

    return previewFor(selected.path, previewData);
  });

  onMount(() => {
    homeDir()
      .then((home) => {
        if (!path) path = home;
      })
      .catch(() => {
        if (!path) path = '/';
      });
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

  function matchesForPath(filePath: string) {
    return matches.filter((match) => match.path === filePath);
  }

  function previewFor(filePath: string, filePreview: PreviewState['filePreview']) {
    const fileMatches = matchesForPath(filePath);
    const activeSelection = selected;
    const activeMatchIndex = activeSelection
      ? fileMatches.findIndex((match) => sameMatch(match, activeSelection))
      : -1;

    return {
      filePath,
      filePreview,
      matches: fileMatches,
      activeMatchIndex
    };
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
      case 'batch':
        if (searchState === 'stopping') return;
        matches = [...matches, ...event.results].slice(0, 100000);
        selected = selected ?? event.results[0] ?? null;
        break;
      case 'error':
        errorMessage = event.message;
        break;
      case 'finished':
        if (searchState === 'stopping') {
          errorMessage = `Search stopped after ${matches.length} matches.`;
          searchState = 'done';
        } else {
          searchState = errorMessage && matches.length === 0 ? 'error' : 'done';
        }
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

    const searchId = activeSearchId;

    try {
      searchState = 'stopping';
      errorMessage = 'Stopping search...';
      await stopSearch(searchId);

      if (activeSearchId === searchId || errorMessage === 'Stopping search...') {
        searchState = 'done';
        errorMessage = `Search stopped after ${matches.length} matches.`;
        activeSearchId = null;
      }
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

  function clampPreviewWidth(width: number) {
    const workspaceWidth = workspaceElement?.getBoundingClientRect().width ?? 0;
    const scopeWidth = 280;
    const splitterWidth = 8;
    const availableWidth = Math.max(0, workspaceWidth - scopeWidth - splitterWidth);
    const maxPreviewWidth = Math.max(260, availableWidth - 260);

    return Math.min(Math.max(width, 260), maxPreviewWidth);
  }

  function startPreviewResize(event: PointerEvent) {
    if (!workspaceElement) return;

    event.preventDefault();
    isResizingPreview = true;

    const updatePreviewWidth = (moveEvent: PointerEvent) => {
      const rect = workspaceElement?.getBoundingClientRect();
      if (!rect) return;
      previewWidth = clampPreviewWidth(rect.right - moveEvent.clientX);
    };

    const stopPreviewResize = () => {
      isResizingPreview = false;
      window.removeEventListener('pointermove', updatePreviewWidth);
      window.removeEventListener('pointerup', stopPreviewResize);
    };

    window.addEventListener('pointermove', updatePreviewWidth);
    window.addEventListener('pointerup', stopPreviewResize, { once: true });
  }

  $effect(() => {
    if (!selected) {
      loadedPreviewKey = '';
      previewLoadId += 1;
      previewError = '';
      previewData = null;
      return;
    }

    const filePath = selected.path;
    const previewKey = `${filePath}:${selected.line_number}:${JSON.stringify(selected.submatches)}`;

    if (loadedPreviewKey === previewKey) {
      return;
    }

    const loadId = ++previewLoadId;
    loadedPreviewKey = previewKey;
    previewError = '';
    previewData = null;

    readFilePreview(filePath, selected.line_number, selected.submatches)
      .then((filePreview) => {
        if (loadId !== previewLoadId || selected?.path !== filePath) return;
        previewData = filePreview;
      })
      .catch((error) => {
        if (loadId !== previewLoadId || selected?.path !== filePath) return;
        previewError = normalizeError(error);
      });
  });
</script>

<svelte:head>
  <title>Searchmonkey III</title>
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

  <div
    bind:this={workspaceElement}
    class:resizing={isResizingPreview}
    class="workspace"
    style:grid-template-columns={`280px minmax(260px, 1fr) 8px minmax(260px, ${previewWidth}px)`}
  >
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
    <button
      type="button"
      aria-label="Resize results and preview panels"
      class="panel-resizer"
      onpointerdown={startPreviewResize}
    ></button>
    <PreviewPanel
      {preview}
      errorMessage={previewError}
      total={matches.length}
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
    min-height: 0;
  }

  .workspace.resizing {
    cursor: col-resize;
    user-select: none;
  }

  .workspace.resizing :global(*) {
    user-select: none;
  }

  .panel-resizer {
    width: 8px;
    min-width: 8px;
    height: 100%;
    border: 0;
    border-left: 1px solid var(--border);
    border-right: 1px solid var(--border);
    border-radius: 0;
    padding: 0;
    background: var(--surface);
    cursor: col-resize;
  }

  .panel-resizer:hover,
  .panel-resizer:focus-visible {
    background: var(--selection);
    outline: none;
  }
</style>
