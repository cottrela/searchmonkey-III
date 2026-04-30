<script lang="ts">
  import { onMount } from 'svelte';
  import { openPath, revealItemInDir } from '@tauri-apps/plugin-opener';
  import PreviewPanel from '$lib/components/PreviewPanel.svelte';
  import RegexTester from '$lib/components/RegexTester.svelte';
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
  import { normalizeExcludePatterns, normalizeIncludePatterns } from '$lib/patterns';
  import { defaultSearchOptions } from '$lib/types';
  import type {
    FileResultGroup,
    FilePreview,
    PreviewState,
    SearchCriteria,
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
  let options = $state<SearchOptions>(defaultSearchOptions());
  let recentSearches = $state<SearchCriteria[]>([]);
  let savedSearches = $state<SearchCriteria[]>([]);

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
  let previewIsLoading = false;
  let previewViewport = $state<{ path: string; start: number; end: number } | null>(null);
  let workspaceElement = $state<HTMLElement>();
  let previewWidth = $state(360);
  let isResizingPreview = $state(false);
  let filtersOpen = $state(false);
  let regexTesterOpen = $state(false);
  let saveDialogOpen = $state(false);
  let saveSearchName = $state('');
  let saveIncludeFilters = $state(true);
  let saveIncludePath = $state(true);
  let saveIncludeOptions = $state(true);
  let compactView = $state<'results' | 'preview'>('results');
  let elapsedMs = $state(0);
  let queuedMatches: SearchMatch[] = [];
  let resultFlushTimer: ReturnType<typeof setTimeout> | null = null;
  let searchStartedAt = 0;
  let elapsedTimer: ReturnType<typeof setInterval> | null = null;
  let resizeFrame = 0;
  let pendingPreviewWidth = 0;
  let scopePanelVisible = true;

  const PREVIEW_CONTEXT_LINES = 50;
  const PREVIEW_EDGE_MARGIN = 10;
  const PREVIEW_LOAD_TIMEOUT_MS = 4000;
  const SEARCH_RESULT_FLUSH_MS = 120;
  const SEARCH_RESULT_FLUSH_WHILE_PREVIEW_LOADING_MS = 300;
  const MAX_DISPLAYED_MATCHES = 10000;
  const RECENT_SEARCHES_KEY = 'searchmonkey:recent-searches';
  const SAVED_SEARCHES_KEY = 'searchmonkey:saved-searches';
  const FILE_TYPE_PATTERNS: Record<string, string[]> = {
    text: ['*.txt', '*.md', '*.markdown', '*.rst', '*.csv', '*.tsv', '*.json', '*.yaml', '*.yml', '*.toml', '*.xml'],
    code: [
      '*.c',
      '*.cc',
      '*.cpp',
      '*.cs',
      '*.css',
      '*.go',
      '*.java',
      '*.js',
      '*.jsx',
      '*.kt',
      '*.php',
      '*.py',
      '*.rb',
      '*.rs',
      '*.svelte',
      '*.swift',
      '*.ts',
      '*.tsx',
      '*.vue'
    ],
    logs: ['*.log', '*.out', '*.err', '*.trace']
  };

  const groups = $derived.by(() => sortGroups(groupMatches(matches), options.sort_by, options.sort_direction));
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
        activeMatchIndex: -1,
        activeMatch: null
      } satisfies PreviewState;
    }

    return previewFor(selected.path, previewData);
  });
  const scopeSummary = $derived.by(() => ({
    folder: path.trim() || 'No folder selected',
    include: includePatterns.trim() || 'all files',
    exclude: excludePatterns.trim()
  }));
  const regexSamples = $derived(matches.slice(0, 300));

  onMount(() => {
    recentSearches = loadCriteria(RECENT_SEARCHES_KEY);
    savedSearches = loadCriteria(SAVED_SEARCHES_KEY);

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

  function sortGroups(
    resultGroups: FileResultGroup[],
    sortBy: SearchOptions['sort_by'],
    direction: SearchOptions['sort_direction']
  ) {
    const nextGroups = resultGroups.map((group) => ({ ...group, matches: [...group.matches] }));
    const multiplier = direction === 'desc' ? -1 : 1;

    if (sortBy === 'file_name') {
      return nextGroups.sort((a, b) => multiplier * filename(a.path).localeCompare(filename(b.path)));
    }

    if (sortBy === 'modified_date') {
      return nextGroups.sort(
        (a, b) => multiplier * ((a.matches[0]?.modified_secs ?? 0) - (b.matches[0]?.modified_secs ?? 0))
      );
    }

    if (sortBy === 'match_count') {
      return nextGroups.sort(
        (a, b) => multiplier * (a.matches.length - b.matches.length) || a.path.localeCompare(b.path)
      );
    }

    return direction === 'desc' ? nextGroups : nextGroups.reverse();
  }

  function filename(filePath: string) {
    const parts = filePath.split('/').filter(Boolean);
    return parts.at(-1) || filePath;
  }

  function includePatternsForType() {
    if (options.file_type === 'all') return [];
    if (options.file_type === 'custom') {
      return options.custom_file_type
        .split(',')
        .map((pattern) => pattern.trim())
        .flatMap((pattern) => customFileTypePattern(pattern));
    }

    return FILE_TYPE_PATTERNS[options.file_type] ?? [];
  }

  function customFileTypePattern(pattern: string) {
    if (!pattern) return [];
    if (pattern.includes('*')) return [pattern];

    const mimePatterns: Record<string, string[]> = {
      'application/json': ['*.json'],
      'application/javascript': ['*.js'],
      'application/typescript': ['*.ts'],
      'application/xml': ['*.xml'],
      'text/plain': ['*.txt'],
      'text/markdown': ['*.md', '*.markdown'],
      'text/csv': ['*.csv'],
      'text/tab-separated-values': ['*.tsv'],
      'text/x-python': ['*.py'],
      'text/x-rust': ['*.rs'],
      'text/x-go': ['*.go']
    };

    if (pattern === 'text/*') return FILE_TYPE_PATTERNS.text;
    if (pattern === 'application/*') return ['*.json', '*.js', '*.ts', '*.xml', '*.toml', '*.yaml', '*.yml'];

    return mimePatterns[pattern.toLowerCase()] ?? [];
  }

  function searchModeRegex() {
    return options.search_mode === 'regex' || options.regex;
  }

  function setSearchMode(mode: SearchOptions['search_mode']) {
    options.search_mode = mode;
    options.regex = mode === 'regex';
  }

  function currentCriteria(
    name = query.trim() || filename(path.trim()) || 'Untitled search',
    settings = { includeFilters: true, includePath: true, includeOptions: true }
  ): SearchCriteria {
    return {
      id: `${Date.now()}:${Math.random().toString(16).slice(2)}`,
      name,
      query,
      path: settings.includePath ? path : '',
      includePatterns: settings.includeFilters ? includePatterns : '',
      excludePatterns: settings.includeFilters ? excludePatterns : '',
      options: settings.includeOptions ? snapshotSearchOptions() : defaultSearchOptions()
    };
  }

  function snapshotSearchOptions(): SearchOptions {
    return {
      regex: options.regex,
      case_sensitive: options.case_sensitive,
      hidden: options.hidden,
      follow_symlinks: options.follow_symlinks,
      multiline: options.multiline,
      context_lines: options.context_lines,
      min_file_size: options.min_file_size,
      max_file_size: options.max_file_size,
      modified_after: options.modified_after,
      skip_binary: options.skip_binary,
      encoding: options.encoding,
      max_matches: options.max_matches,
      respect_gitignore: options.respect_gitignore,
      ignore_node_modules: options.ignore_node_modules,
      ignore_build_artifacts: options.ignore_build_artifacts,
      search_mode: options.search_mode,
      modified_preset: options.modified_preset,
      modified_custom_days: options.modified_custom_days,
      file_type: options.file_type,
      custom_file_type: options.custom_file_type,
      sort_by: options.sort_by,
      sort_direction: options.sort_direction,
      show_line_numbers: options.show_line_numbers,
      group_by_file: options.group_by_file
    };
  }

  function applyCriteria(criteria: SearchCriteria | undefined) {
    if (!criteria) return;

    query = criteria.query;
    path = criteria.path;
    includePatterns = criteria.includePatterns;
    excludePatterns = criteria.excludePatterns;
    options = { ...defaultSearchOptions(), ...criteria.options };
    contextLines = options.context_lines;
  }

  function openSaveDialog() {
    saveSearchName = query.trim() || filename(path.trim()) || 'Untitled search';
    saveIncludeFilters = true;
    saveIncludePath = true;
    saveIncludeOptions = true;
    saveDialogOpen = true;
  }

  function saveCurrentCriteria() {
    const cleanName = saveSearchName.trim() || query.trim() || filename(path.trim()) || 'Untitled search';
    const criteria = currentCriteria(cleanName, {
      includeFilters: saveIncludeFilters,
      includePath: saveIncludePath,
      includeOptions: saveIncludeOptions
    });
    savedSearches = [criteria, ...savedSearches.filter((search) => search.name !== criteria.name)].slice(0, 20);
    saveCriteria(SAVED_SEARCHES_KEY, savedSearches);
    saveDialogOpen = false;
  }

  function renameSavedSearch(criteria: SearchCriteria) {
    const nextName = window.prompt('Rename saved search', criteria.name)?.trim();
    if (!nextName) return;

    savedSearches = savedSearches.map((search) => (search.id === criteria.id ? { ...search, name: nextName } : search));
    saveCriteria(SAVED_SEARCHES_KEY, savedSearches);
  }

  function deleteSavedSearch(criteria: SearchCriteria) {
    if (!window.confirm(`Delete "${criteria.name}"?`)) return;

    savedSearches = savedSearches.filter((search) => search.id !== criteria.id);
    saveCriteria(SAVED_SEARCHES_KEY, savedSearches);
  }

  function rememberRecentSearch() {
    const criteria = currentCriteria();
    recentSearches = [
      criteria,
      ...recentSearches.filter(
        (search) =>
          search.query !== criteria.query ||
          search.path !== criteria.path ||
          search.includePatterns !== criteria.includePatterns ||
          search.excludePatterns !== criteria.excludePatterns
      )
    ].slice(0, 12);
    saveCriteria(RECENT_SEARCHES_KEY, recentSearches);
  }

  function loadCriteria(key: string) {
    try {
      const parsed = JSON.parse(localStorage.getItem(key) ?? '[]');
      if (!Array.isArray(parsed)) return [];
      return parsed.map((criteria) => ({
        ...criteria,
        options: { ...defaultSearchOptions(), ...criteria.options }
      }));
    } catch {
      return [];
    }
  }

  function saveCriteria(key: string, criteria: SearchCriteria[]) {
    localStorage.setItem(key, JSON.stringify(criteria));
  }

  function sameMatch(a: SearchMatch, b: SearchMatch) {
    return a.path === b.path && a.line_number === b.line_number && a.line_text === b.line_text;
  }

  function previewFor(filePath: string, filePreview: PreviewState['filePreview']) {
    const activeSelection = selected;
    const viewportStart = filePreview?.start_line ?? previewViewport?.start ?? 0;
    const viewportEnd = filePreview?.end_line ?? previewViewport?.end ?? 0;
    const visibleMatches =
      viewportStart && viewportEnd
        ? matches.filter(
            (match) =>
              match.path === filePath &&
              match.line_number >= viewportStart &&
              match.line_number <= viewportEnd
          )
        : [];

    return {
      filePath,
      filePreview,
      matches: visibleMatches,
      activeMatchIndex: selectedIndex,
      activeMatch: activeSelection
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
        startElapsedTimer();
        break;
      case 'batch':
        if (searchState === 'stopping') return;
        queueMatches(event.results);
        break;
      case 'error':
        errorMessage = event.message;
        break;
      case 'finished':
        flushQueuedMatches();
        if (searchState === 'stopping') {
          errorMessage = `Search stopped after ${matches.length} matches.`;
          searchState = 'done';
        } else {
          searchState = errorMessage && matches.length === 0 ? 'error' : 'done';
        }
        activeSearchId = null;
        stopElapsedTimer();
        break;
      case 'cancelled':
        flushQueuedMatches();
        searchState = 'done';
        errorMessage = `Search stopped after ${matches.length} matches.`;
        activeSearchId = null;
        stopElapsedTimer();
        break;
    }
  }

  async function startSearch() {
    if (searchState === 'searching' || searchState === 'stopping') return;

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
    elapsedMs = 0;
    startElapsedTimer();
    hasSearched = true;
    selected = null;
    matches = [];
    queuedMatches = [];
    clearResultFlushTimer();
    const searchId = nextSearchId;
    nextSearchId += 1;
    activeSearchId = searchId;
    rememberRecentSearch();

    try {
      const includeFromType = includePatternsForType();
      await startSearchCommand(
        {
          query: cleanQuery,
          path: cleanPath,
          regex: searchModeRegex(),
          case_sensitive: options.case_sensitive,
          hidden: options.hidden,
          include_patterns: [...normalizeIncludePatterns(includePatterns), ...includeFromType],
          exclude_patterns: normalizeExcludePatterns(excludePatterns),
          follow_symlinks: options.follow_symlinks,
          multiline: options.multiline,
          context_lines: options.context_lines,
          min_file_size: options.min_file_size,
          max_file_size: options.max_file_size,
          modified_after: options.modified_after,
          skip_binary: options.skip_binary,
          encoding: options.encoding,
          max_matches: options.max_matches,
          respect_gitignore: options.respect_gitignore,
          ignore_node_modules: options.ignore_node_modules,
          ignore_build_artifacts: options.ignore_build_artifacts
        },
        searchId,
        handleSearchEvent
      );
    } catch (error) {
      matches = [];
      queuedMatches = [];
      clearResultFlushTimer();
      selected = null;
      activeSearchId = null;
      searchState = 'error';
      stopElapsedTimer();
      errorMessage = normalizeError(error);
    }
  }

  async function stopCurrentSearch() {
    if (searchState !== 'searching' || activeSearchId === null) return;

    const searchId = activeSearchId;

    try {
      searchState = 'stopping';
      errorMessage = '';
      await stopSearch(searchId);

      if (activeSearchId === searchId) {
        searchState = 'done';
        errorMessage = `Search stopped after ${matches.length} matches.`;
        activeSearchId = null;
      }
    } catch (error) {
      searchState = 'error';
      errorMessage = normalizeError(error);
    }
  }

  function handleGlobalKeydown(event: KeyboardEvent) {
    if ((event.metaKey || event.ctrlKey) && event.shiftKey && event.key.toLowerCase() === 'r' && searchModeRegex()) {
      event.preventDefault();
      if (regexTesterOpen) {
        closeRegexTester();
      } else {
        openRegexTester();
      }
      return;
    }

    if (event.key === 'Escape' && searchState === 'searching') {
      event.preventDefault();
      void stopCurrentSearch();
      return;
    }

    if (isEditableTarget(event.target)) return;

    if (event.key === 'Enter') {
      event.preventDefault();
      void startSearch();
      return;
    }

    if (event.key === 'n' && matches.length) {
      event.preventDefault();
      selectOffset(event.shiftKey ? -1 : 1);
      return;
    }

    if (event.key === 'ArrowDown' && matches.length) {
      event.preventDefault();
      selectOffset(1);
      return;
    }

    if (event.key === 'ArrowUp' && matches.length) {
      event.preventDefault();
      selectOffset(-1);
      return;
    }
  }

  function selectMatch(match: SearchMatch) {
    scheduleResultFlush(SEARCH_RESULT_FLUSH_WHILE_PREVIEW_LOADING_MS);
    previewViewport = updateViewportForMatch(match);
    selected = match;
    compactView = 'preview';
  }

  function isEditableTarget(target: EventTarget | null) {
    if (!(target instanceof HTMLElement)) return false;
    const tagName = target.tagName.toLowerCase();
    return tagName === 'input' || tagName === 'textarea' || tagName === 'select' || target.isContentEditable;
  }

  async function openFile(filePath: string) {
    if (!filePath) return;

    try {
      await openPath(filePath);
    } catch (error) {
      errorMessage = normalizeError(error);
    }
  }

  async function revealFile(filePath: string) {
    if (!filePath) return;

    try {
      await revealItemInDir(filePath);
    } catch (error) {
      errorMessage = normalizeError(error);
    }
  }

  function selectOffset(offset: number) {
    if (!matches.length) return;

    const currentIndex = selectedIndex >= 0 ? selectedIndex : 0;
    const nextIndex = (currentIndex + offset + matches.length) % matches.length;
    const nextMatch = matches[nextIndex];
    scheduleResultFlush(SEARCH_RESULT_FLUSH_WHILE_PREVIEW_LOADING_MS);
    previewViewport = updateViewportForMatch(nextMatch);
    selected = nextMatch;
  }

  function queueMatches(nextMatches: SearchMatch[]) {
    if (!nextMatches.length) return;

    const remainingCapacity = MAX_DISPLAYED_MATCHES - matches.length - queuedMatches.length;

    if (remainingCapacity <= 0) {
      return;
    }

    queuedMatches.push(...nextMatches.slice(0, remainingCapacity));
    scheduleResultFlush();
  }

  function scheduleResultFlush(delay = previewIsLoading ? SEARCH_RESULT_FLUSH_WHILE_PREVIEW_LOADING_MS : SEARCH_RESULT_FLUSH_MS) {
    if (!queuedMatches.length) return;

    if (resultFlushTimer) {
      if (delay === 0) {
        clearResultFlushTimer();
      } else {
        return;
      }
    }

    if (delay === 0) {
      flushQueuedMatches();
      return;
    }

    resultFlushTimer = setTimeout(() => {
      resultFlushTimer = null;
      flushQueuedMatches();
    }, delay);
  }

  function flushQueuedMatches() {
    if (!queuedMatches.length) return;

    const nextMatches = queuedMatches;
    queuedMatches = [];
    matches = [...matches, ...nextMatches].slice(0, MAX_DISPLAYED_MATCHES);
  }

  function clearResultFlushTimer() {
    if (!resultFlushTimer) return;

    clearTimeout(resultFlushTimer);
    resultFlushTimer = null;
  }

  function startElapsedTimer() {
    searchStartedAt = Date.now();
    clearElapsedTimer();
    elapsedTimer = setInterval(() => {
      elapsedMs = Date.now() - searchStartedAt;
    }, 100);
  }

  function stopElapsedTimer() {
    if (searchStartedAt) {
      elapsedMs = Date.now() - searchStartedAt;
    }
    clearElapsedTimer();
  }

  function clearElapsedTimer() {
    if (!elapsedTimer) return;
    clearInterval(elapsedTimer);
    elapsedTimer = null;
  }

  function updateViewportForMatch(match: SearchMatch) {
    const currentStart = previewViewport?.path === match.path ? previewViewport.start : 0;
    const currentEnd = previewViewport?.path === match.path ? previewViewport.end : 0;
    const selectedLine = match.line_number;
    const isVisible =
      selectedLine >= currentStart + PREVIEW_EDGE_MARGIN &&
      selectedLine <= currentEnd - PREVIEW_EDGE_MARGIN;

    if (isVisible) {
      return previewViewport;
    }

    const start = Math.max(1, selectedLine - PREVIEW_CONTEXT_LINES);
    const end = selectedLine + PREVIEW_CONTEXT_LINES;

    return { path: match.path, start, end };
  }

  function clampPreviewWidth(width: number) {
    const workspaceWidth = workspaceElement?.getBoundingClientRect().width ?? 0;
    const scopeWidth = scopePanelVisible ? 280 : 0;
    const splitterWidth = 8;
    const availableWidth = Math.max(0, workspaceWidth - scopeWidth - splitterWidth);
    const maxPreviewWidth = Math.max(260, availableWidth - 260);

    return Math.min(Math.max(width, 260), maxPreviewWidth);
  }

  function startPreviewResize(event: PointerEvent) {
    if (!workspaceElement) return;

    event.preventDefault();
    isResizingPreview = true;
    scopePanelVisible = window.matchMedia('(min-width: 1200px)').matches;

    const updatePreviewWidth = (moveEvent: PointerEvent) => {
      const rect = workspaceElement?.getBoundingClientRect();
      if (!rect) return;
      pendingPreviewWidth = clampPreviewWidth(rect.right - moveEvent.clientX);

      if (resizeFrame) return;
      resizeFrame = requestAnimationFrame(() => {
        resizeFrame = 0;
        previewWidth = pendingPreviewWidth;
      });
    };

    const stopPreviewResize = () => {
      isResizingPreview = false;
      if (resizeFrame) {
        cancelAnimationFrame(resizeFrame);
        resizeFrame = 0;
        previewWidth = pendingPreviewWidth;
      }
      window.removeEventListener('pointermove', updatePreviewWidth);
      window.removeEventListener('pointerup', stopPreviewResize);
    };

    window.addEventListener('pointermove', updatePreviewWidth);
    window.addEventListener('pointerup', stopPreviewResize, { once: true });
  }

  function closePreview() {
    compactView = 'results';
  }

  function openRegexTester() {
    regexTesterOpen = true;
    compactView = 'preview';
  }

  function closeRegexTester() {
    regexTesterOpen = false;
    compactView = 'results';
  }

  $effect(() => {
    if (!searchModeRegex()) {
      closeRegexTester();
    }
  });

  $effect(() => {
    if (!selected) {
      loadedPreviewKey = '';
      previewLoadId += 1;
      previewError = '';
      previewData = null;
      previewViewport = null;
      previewIsLoading = false;
      return;
    }

    const filePath = selected.path;
    const nextViewport = updateViewportForMatch(selected);

    if (!nextViewport) return;

    const previewKey = `${filePath}:${nextViewport.start}:${nextViewport.end}`;

    if (loadedPreviewKey === previewKey) {
      return;
    }

    const loadId = ++previewLoadId;
    loadedPreviewKey = previewKey;
    previewError = '';
    previewData = null;
    previewIsLoading = true;

    withTimeout(
      readFilePreview(filePath, nextViewport.start, nextViewport.end),
      PREVIEW_LOAD_TIMEOUT_MS,
      'Preview is taking too long. Search is still usable; try another result or a smaller file.'
    )
      .then((filePreview) => {
        if (loadId !== previewLoadId || selected?.path !== filePath) return;
        previewData = filePreview;
        previewIsLoading = false;
        scheduleResultFlush(0);
      })
      .catch((error) => {
        if (loadId !== previewLoadId || selected?.path !== filePath) return;
        previewError = normalizeError(error);
        previewIsLoading = false;
        scheduleResultFlush(0);
      });
  });

  function withTimeout<T>(promise: Promise<T>, timeoutMs: number, message: string) {
    let timeoutId: ReturnType<typeof setTimeout>;
    const timeout = new Promise<never>((_, reject) => {
      timeoutId = setTimeout(() => reject(new Error(message)), timeoutMs);
    });

    return Promise.race([promise, timeout]).finally(() => clearTimeout(timeoutId));
  }
</script>

<svelte:head>
  <title>Searchmonkey III</title>
</svelte:head>

<svelte:window onkeydown={handleGlobalKeydown} />

<main class="app-shell">
  <SearchBar
    bind:query
    bind:options
    searching={searchState === 'searching' || searchState === 'stopping'}
    stopping={searchState === 'stopping'}
    {savedSearches}
    onFilters={() => (filtersOpen = true)}
    onRegexTester={openRegexTester}
    onApplyCriteria={applyCriteria}
    onSaveRequest={openSaveDialog}
    onRenameCriteria={renameSavedSearch}
    onDeleteCriteria={deleteSavedSearch}
    onSearch={startSearch}
    onStop={stopCurrentSearch}
  />

  <div class="scope-summary" aria-label="Search scope summary">
    <span class="summary-item folder" title={scopeSummary.folder}>
      <strong>Folder:</strong> {scopeSummary.folder}
    </span>
    <span class="summary-separator" aria-hidden="true">·</span>
    <span class="summary-item include"><strong>Include:</strong> {scopeSummary.include}</span>
    {#if scopeSummary.exclude}
      <span class="summary-separator exclude-separator" aria-hidden="true">·</span>
      <span class="summary-item exclude"><strong>Exclude:</strong> {scopeSummary.exclude}</span>
    {/if}
    <div class="mode-pills" aria-label="Search modes">
      <button
        type="button"
        class:active={options.search_mode === 'regex'}
        onclick={() => setSearchMode(options.search_mode === 'regex' ? 'literal' : 'regex')}
      >
        Regex
      </button>
      <button
        type="button"
        class:active={options.case_sensitive}
        onclick={() => (options.case_sensitive = !options.case_sensitive)}
      >
        Case
      </button>
      <button type="button" class:active={options.hidden} onclick={() => (options.hidden = !options.hidden)}>
        Hidden
      </button>
    </div>
  </div>

  <div class="results-toolbar" aria-label="Results actions">
    <span>{groups.length} files</span>
    <span>{matches.length} matches</span>
    <button type="button" onclick={() => (filtersOpen = true)}>Filters</button>
    <button type="button" disabled={!selected} onclick={() => (compactView = 'preview')}>Preview</button>
  </div>

  <div
    bind:this={workspaceElement}
    class:resizing={isResizingPreview}
    class:has-preview={Boolean(selected) || regexTesterOpen}
    class:show-preview={compactView === 'preview' || regexTesterOpen}
    class="workspace"
    style:--preview-width={`${previewWidth}px`}
    style:grid-template-columns={`280px minmax(260px, 1fr) 8px minmax(260px, var(--preview-width))`}
  >
    <ScopePanel
      bind:path
      bind:includePatterns
      bind:excludePatterns
      bind:contextLines
      bind:options
      includeHidden={options.hidden}
    />
    <ResultsPanel
      {groups}
      {query}
      regex={searchModeRegex()}
      bind:options
      {selected}
      state={searchState}
      {hasSearched}
      onSelect={selectMatch}
      onOpen={openFile}
      onReveal={revealFile}
    />
    <button
      type="button"
      aria-label="Resize results and preview panels"
      class="panel-resizer"
      onpointerdown={startPreviewResize}
    ></button>
    {#if regexTesterOpen && searchModeRegex()}
      <RegexTester
        bind:query
        bind:options
        samples={regexSamples}
        onClose={closeRegexTester}
      />
    {:else}
      <PreviewPanel
        {preview}
        errorMessage={previewError}
        total={matches.length}
        onPrevious={() => selectOffset(-1)}
        onNext={() => selectOffset(1)}
        onSelect={selectMatch}
        onOpen={openFile}
        onReveal={revealFile}
        onClose={closePreview}
      />
    {/if}
  </div>

  {#if filtersOpen}
    <div class="drawer-layer" role="presentation">
      <button
        class="drawer-backdrop"
        type="button"
        aria-label="Close filters"
        onclick={() => (filtersOpen = false)}
      ></button>
      <div
        class="filters-drawer"
        role="dialog"
        aria-modal="true"
        aria-label="Search filters"
        tabindex="-1"
      >
        <div class="drawer-header">
          <h2>Filters</h2>
          <button type="button" onclick={() => (filtersOpen = false)}>Close</button>
        </div>
        <ScopePanel
          bind:path
          bind:includePatterns
          bind:excludePatterns
          bind:contextLines
          bind:options
          includeHidden={options.hidden}
        />
      </div>
    </div>
  {/if}

  {#if saveDialogOpen}
    <div class="modal-layer" role="presentation">
      <button
        class="modal-backdrop"
        type="button"
        aria-label="Close save search"
        onclick={() => (saveDialogOpen = false)}
      ></button>
      <div class="save-dialog" role="dialog" aria-modal="true" aria-label="Save search">
        <form onsubmit={(event) => { event.preventDefault(); saveCurrentCriteria(); }}>
          <header>
            <h2>Save Search</h2>
            <button type="button" onclick={() => (saveDialogOpen = false)}>Close</button>
          </header>
          <div class="save-body">
            <div class="field">
              <label for="saved-search-name">Save search as</label>
              <input id="saved-search-name" type="text" bind:value={saveSearchName} placeholder="Name" autocomplete="off" />
            </div>
            <label class="check-row">
              <input type="checkbox" bind:checked={saveIncludeFilters} />
              <span>Include filters</span>
            </label>
            <label class="check-row">
              <input type="checkbox" bind:checked={saveIncludePath} />
              <span>Include path</span>
            </label>
            <label class="check-row">
              <input type="checkbox" bind:checked={saveIncludeOptions} />
              <span>Include options</span>
            </label>
          </div>
          <footer>
            <button class="primary-save" type="submit">Save</button>
          </footer>
        </form>
      </div>
    </div>
  {/if}

  <StatusBar
    state={searchState}
    totalMatches={matches.length}
    filesWithMatches={groups.length}
    {elapsedMs}
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

  .scope-summary,
  .results-toolbar {
    display: none;
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

  .drawer-layer {
    position: fixed;
    inset: 0;
    z-index: 20;
  }

  .modal-layer {
    position: fixed;
    inset: 0;
    z-index: 35;
    display: grid;
    place-items: center;
    padding: 18px;
  }

  .drawer-backdrop,
  .modal-backdrop {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    border: 0;
    padding: 0;
    background: rgba(30, 37, 45, 0.24);
  }

  .save-dialog {
    position: relative;
    z-index: 1;
    width: min(360px, calc(100vw - 36px));
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
    background: var(--panel);
    box-shadow: 0 18px 42px rgba(30, 37, 45, 0.22);
  }

  .save-dialog form {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
  }

  .save-dialog header,
  .save-dialog footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    border-bottom: 1px solid var(--border);
    padding: 10px 12px;
    background: var(--surface);
  }

  .save-dialog footer {
    justify-content: flex-end;
    border-top: 1px solid var(--border);
    border-bottom: 0;
  }

  .save-dialog h2 {
    margin: 0;
    font-size: 14px;
  }

  .save-body {
    display: grid;
    gap: 10px;
    padding: 12px;
  }

  .save-body .field {
    display: grid;
    gap: 5px;
  }

  .save-body label,
  .save-body .check-row span {
    color: var(--muted);
    font-size: 12px;
    font-weight: 700;
  }

  .save-body input[type='text'],
  .save-body input:not([type]) {
    width: 100%;
    height: 34px;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    padding: 0 9px;
    color: var(--text);
    background: var(--input);
    font: inherit;
    font-size: 12px;
  }

  .save-body input:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--focus);
    outline: none;
  }

  .save-body .check-row {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 8px;
    align-items: center;
  }

  .save-dialog button {
    height: 30px;
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0 9px;
    color: var(--text);
    background: var(--input);
    font: inherit;
    font-size: 12px;
    font-weight: 750;
    cursor: pointer;
  }

  .save-dialog .primary-save {
    border-color: var(--accent);
    color: #ffffff;
    background: var(--accent);
  }

  .filters-drawer {
    position: relative;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    width: min(360px, calc(100vw - 32px));
    height: 100%;
    border-right: 1px solid var(--border);
    background: var(--panel);
    box-shadow: 0 14px 36px rgba(30, 37, 45, 0.22);
  }

  .drawer-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 44px;
    border-bottom: 1px solid var(--border);
    padding: 0 12px;
    background: var(--surface);
  }

  .drawer-header h2 {
    margin: 0;
    font-size: 14px;
  }

  .results-toolbar button,
  .drawer-header button {
    height: 28px;
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0 9px;
    color: var(--text);
    background: var(--input);
    font: inherit;
    font-size: 12px;
    font-weight: 750;
  }

  .results-toolbar button:not(:disabled),
  .drawer-header button:not(:disabled) {
    cursor: pointer;
  }

  .results-toolbar button:disabled {
    color: var(--muted);
    background: var(--disabled);
  }

  @media (max-width: 1199px) {
    .app-shell {
      grid-template-rows: auto auto minmax(0, 1fr) auto;
    }

    .scope-summary {
      display: grid;
      grid-template-columns:
        minmax(160px, 1.4fr) auto minmax(120px, 0.8fr) auto minmax(120px, 0.8fr)
        auto;
      gap: 8px;
      align-items: center;
      min-height: 36px;
      border-bottom: 1px solid var(--border);
      padding: 5px 12px;
      color: var(--muted);
      background: var(--panel);
      font-size: 12px;
      font-weight: 650;
    }

    .summary-item {
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .summary-item strong {
      color: var(--text);
    }

    .summary-separator {
      color: var(--muted);
      font-weight: 800;
    }

    .mode-pills {
      display: flex;
      gap: 5px;
      justify-content: flex-end;
    }

    .mode-pills button {
      height: 24px;
      border: 1px solid var(--border-subtle);
      border-radius: 999px;
      padding: 0 9px;
      color: var(--muted);
      background: var(--surface);
      font: inherit;
      font-size: 11px;
      font-weight: 800;
    }

    .mode-pills button.active {
      border-color: var(--accent-soft);
      color: var(--text);
      background: var(--selection);
    }

    .workspace {
      grid-template-columns: minmax(260px, 1fr) 8px minmax(260px, var(--preview-width)) !important;
    }

    .workspace > :global(.scope-panel) {
      display: none;
    }
  }

  @media (max-width: 849px) {
    .workspace {
      grid-template-columns: minmax(0, 1fr) !important;
      grid-template-rows: minmax(0, 1fr);
    }

    .workspace > :global(.results-panel),
    .workspace > :global(.preview-panel),
    .workspace > :global(.regex-panel) {
      grid-column: 1;
      grid-row: 1;
      min-height: 0;
    }

    .workspace > :global(.preview-panel),
    .workspace > :global(.regex-panel),
    .workspace.show-preview > :global(.results-panel),
    .panel-resizer {
      display: none;
    }

    .workspace.show-preview > :global(.preview-panel),
    .workspace.show-preview > :global(.regex-panel) {
      display: grid;
    }
  }

  @media (max-width: 640px) {
    .scope-summary {
      grid-template-columns: minmax(0, 1fr) auto minmax(90px, 0.7fr) auto;
    }

    .exclude,
    .exclude-separator {
      display: none;
    }
  }

  @media (max-width: 599px) {
    .app-shell {
      grid-template-rows: auto auto minmax(0, 1fr) auto;
    }

    .scope-summary {
      grid-template-columns: minmax(0, 1fr);
      min-height: 28px;
      padding: 3px 8px;
    }

    .summary-separator,
    .include,
    .exclude,
    .mode-pills {
      display: none;
    }

    .results-toolbar {
      display: none;
    }
  }
</style>
