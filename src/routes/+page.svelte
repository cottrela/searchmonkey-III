<script lang="ts">
  import { onMount } from 'svelte';
  import { getVersion } from '@tauri-apps/api/app';
  import { listen } from '@tauri-apps/api/event';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import PathInput from '$lib/components/PathInput.svelte';
  import AboutDialog from '$lib/components/AboutDialog.svelte';
  import PreviewPanel from '$lib/components/PreviewPanel.svelte';
  import RegexTester from '$lib/components/RegexTester.svelte';
  import ResultsPanel from '$lib/components/ResultsPanel.svelte';
  import ScopePanel from '$lib/components/ScopePanel.svelte';
  import SearchBar from '$lib/components/SearchBar.svelte';
  import StatusBar from '$lib/components/StatusBar.svelte';
  import TelemetryConsentDialog from '$lib/components/TelemetryConsentDialog.svelte';
  import {
    cancelSearch as cancelSearchCommand,
    getResults,
    getSearchStatus,
    homeDir,
    listenSearchBufferUpdated,
    listenSearchStatusChanged,
    openFilePath,
    readFilePreview,
    revealFilePath,
    startSearch as startSearchCommand
  } from '$lib/search';
  import { normalizeExcludePatterns, normalizeIncludePatterns } from '$lib/patterns';
  import { filename, normalizeGlobPattern, parentPath } from '$lib/paths';
  import { loadTelemetryState, syncTelemetryConsent, type TelemetryState } from '$lib/telemetry';
  import { getAvailableUpdate, type AvailableUpdate } from '$lib/update-check';
  import { defaultSearchOptions } from '$lib/types';
  import type {
    FileResultGroup,
    FilePreview,
    PreviewState,
    SearchBufferUpdatedEvent,
    SearchCriteria,
    SearchMatch,
    SearchOptions,
    SearchStatusChangedEvent,
    SearchState
  } from '$lib/types';

  let query = $state('');
  let path = $state('');
  let includePatterns = $state<string[]>([]);
  let excludePatterns = $state<string[]>([]);
  let contextLines = $state(0);
  let options = $state<SearchOptions>(defaultSearchOptions());
  let recentSearches = $state<SearchCriteria[]>([]);
  let savedSearches = $state<SearchCriteria[]>([]);

  let matches: SearchMatch[] = [];
  let matchesVersion = $state(0);
  let selected = $state<SearchMatch | null>(null);
  let searchState = $state<SearchState>('idle');
  let errorMessage = $state('');
  let hasSearched = $state(false);
  let activeSearchId = $state<number | null>(null);
  let searchUnlisteners: Array<() => void> = [];
  let improveMenuEventUnlisten: (() => void) | null = null;
  let aboutMenuEventUnlisten: (() => void) | null = null;
  let releaseNotesMenuEventUnlisten: (() => void) | null = null;
  let websiteMenuEventUnlisten: (() => void) | null = null;
  let reportIssueMenuEventUnlisten: (() => void) | null = null;
  let checkForUpdatesMenuEventUnlisten: (() => void) | null = null;
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
  let defaultHomePath = '';
  let aboutDialogOpen = $state(false);
  let telemetryState = $state<TelemetryState | null>(null);
  let telemetryDialogOpen = $state(false);
  let telemetryFirstRun = $state(false);
  let availableUpdate = $state<AvailableUpdate | null>(null);
  let compactView = $state<'results' | 'preview'>('results');
  let layoutMode = $state<'focus' | 'split' | 'full'>('split');
  let oneUpConstrained = $state(false);
  let fullModeAvailable = $state(false);
  let elapsedMs = $state(0);
  let resultFlushTimer: ReturnType<typeof setTimeout> | null = null;
  let searchStatusPollTimer: ReturnType<typeof setInterval> | null = null;
  let pendingMatchesRender = false;
  let searchStartedAt = 0;
  let elapsedTimer: ReturnType<typeof setInterval> | null = null;
  let backendMatchCount = $state(0);
  let resultPullPromise: Promise<void> | null = null;
  let pendingResultPull = false;
  let statusPollInFlight = false;
  let finishingSearchId: number | null = null;
  let resizeFrame = 0;
  let pendingPreviewWidth = 0;
  let scopePanelVisible = true;

  const PREVIEW_CONTEXT_LINES = 50;
  const PREVIEW_EDGE_MARGIN = 10;
  const PREVIEW_LOAD_TIMEOUT_MS = 4000;
  const SEARCH_RESULT_FLUSH_MS = 500;
  const SEARCH_RESULT_FLUSH_WHILE_PREVIEW_LOADING_MS = 750;
  const SEARCH_STATUS_POLL_MS = 150;
  const MAX_DISPLAYED_MATCHES = 100000;
  const RECENT_SEARCHES_KEY = 'searchmonkey:recent-searches';
  const SAVED_SEARCHES_KEY = 'searchmonkey:saved-searches';
  const DISMISSED_UPDATE_KEY = 'searchmonkey:dismissed-update';
  const UPDATE_DISMISS_MS = 3 * 24 * 60 * 60 * 1000;
  const RELEASE_NOTES_URL = 'https://github.com/cottrela/searchmonkey-v3/releases';
  const WEBSITE_URL = 'https://searchmonkey.dev';
  const REPORT_ISSUE_URL = 'https://github.com/cottrela/searchmonkey-v3/issues';
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

  const displayedMatchCount = $derived.by(() => {
    matchesVersion;
    return matches.length;
  });
  const groups = $derived.by(() => {
    matchesVersion;
    return sortGroups(groupMatches(matches), options.sort_by, options.sort_direction);
  });
  const displayedMatches = $derived.by(() => {
    matchesVersion;
    return groups.flatMap((group) => group.matches);
  });
  const selectedIndex = $derived.by(() => {
    matchesVersion;
    if (!selected) return -1;
    const current = selected;
    return displayedMatches.findIndex((match) => sameMatch(match, current));
  });
  const selectedFileMatchIndex = $derived.by(() => {
    matchesVersion;
    if (!selected) return -1;
    const current = selected;
    const group = groups.find((resultGroup) => resultGroup.path === current.path);
    return group?.matches.findIndex((match) => sameMatch(match, current)) ?? -1;
  });
  const selectedFileMatchCount = $derived.by(() => {
    matchesVersion;
    if (!selected) return 0;
    const current = selected;
    return groups.find((resultGroup) => resultGroup.path === current.path)?.matches.length ?? 0;
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
  const regexSamples = $derived.by(() => {
    matchesVersion;
    return matches.slice(0, 300);
  });
  const activeLayoutMode = $derived(oneUpConstrained ? 'focus' : layoutMode === 'full' && !fullModeAvailable ? 'split' : layoutMode);
  const availableLayoutModes = $derived.by(() => {
    if (oneUpConstrained) return ['focus'] as const;
    if (fullModeAvailable) return ['focus', 'split', 'full'] as const;
    return ['focus', 'split'] as const;
  });
  const scopePanelVisibleInLayout = $derived(activeLayoutMode === 'full');
  const workspaceGridTemplate = $derived.by(() => {
    if (activeLayoutMode === 'full') {
      return `280px minmax(360px, 3fr) 8px minmax(300px, var(--preview-width))`;
    }

    if (activeLayoutMode === 'split') {
      return `minmax(360px, 3fr) 8px minmax(300px, var(--preview-width))`;
    }

    return 'minmax(0, 1fr)';
  });

  function resetMatches() {
    matches = [];
    pendingMatchesRender = false;
    clearResultFlushTimer();
    matchesVersion += 1;
  }

  function appendMatches(nextMatches: SearchMatch[], immediateRender = false) {
    if (!nextMatches.length || matches.length >= MAX_DISPLAYED_MATCHES) return;

    const remainingCapacity = MAX_DISPLAYED_MATCHES - matches.length;
    const keptMatches = nextMatches.length > remainingCapacity ? nextMatches.slice(0, remainingCapacity) : nextMatches;

    for (let index = 0; index < keptMatches.length; index += 1000) {
      matches.push(...keptMatches.slice(index, index + 1000));
    }

    pendingMatchesRender = true;
    if (!immediateRender) {
      scheduleResultFlush();
      return;
    }

    flushQueuedMatches();
  }

  function renderPendingMatches() {
    if (!pendingMatchesRender) return;

    pendingMatchesRender = false;
    matchesVersion += 1;
  }

  type DismissedUpdate = {
    tagName: string;
    dismissedUntil: number;
  };

  function dismissedUpdate(): DismissedUpdate | null {
    const storedDismissal = localStorage.getItem(DISMISSED_UPDATE_KEY);
    if (!storedDismissal) return null;

    try {
      const dismissal = JSON.parse(storedDismissal) as Partial<DismissedUpdate>;
      if (typeof dismissal.tagName !== 'string' || typeof dismissal.dismissedUntil !== 'number') {
        localStorage.removeItem(DISMISSED_UPDATE_KEY);
        return null;
      }

      return {
        tagName: dismissal.tagName,
        dismissedUntil: dismissal.dismissedUntil
      };
    } catch {
      localStorage.removeItem(DISMISSED_UPDATE_KEY);
      return null;
    }
  }

  function updateIsDismissed(tagName: string) {
    const dismissal = dismissedUpdate();
    if (!dismissal || dismissal.tagName !== tagName) return false;
    if (dismissal.dismissedUntil > Date.now()) return true;

    localStorage.removeItem(DISMISSED_UPDATE_KEY);
    return false;
  }

  async function checkForAvailableUpdate(manual = false) {
    try {
      const currentVersion = await getVersion();
      const update = await getAvailableUpdate(currentVersion);
      if (!update) {
        if (manual) {
          availableUpdate = null;
          errorMessage = `Searchmonkey ${currentVersion} is up to date.`;
        }
        return;
      }

      if (!manual && updateIsDismissed(update.tagName)) {
        return;
      }

      if (manual) {
        localStorage.removeItem(DISMISSED_UPDATE_KEY);
        errorMessage = '';
      }
      availableUpdate = update;
    } catch (error) {
      console.warn('[searchmonkey] update check failed', error);
      if (manual) {
        errorMessage = normalizeError(error);
      }
    }
  }

  function dismissUpdate() {
    if (availableUpdate) {
      localStorage.setItem(
        DISMISSED_UPDATE_KEY,
        JSON.stringify({
          tagName: availableUpdate.tagName,
          dismissedUntil: Date.now() + UPDATE_DISMISS_MS
        } satisfies DismissedUpdate)
      );
    }
    availableUpdate = null;
  }

  function openUpdateDownload() {
    if (!availableUpdate) return;
    void openUrl(availableUpdate.downloadUrl).catch(() => {
      if (availableUpdate) {
        void openUrl(availableUpdate.releaseUrl).catch(() => {});
      }
    });
  }

  function openUpdateReleaseNotes() {
    if (!availableUpdate) return;
    void openUrl(availableUpdate.releaseUrl).catch(() => {});
  }

  onMount(() => {
    const oneUpMedia = window.matchMedia('(max-width: 849px)');
    const fullMedia = window.matchMedia('(min-width: 1100px)');
    const syncConstraint = () => {
      oneUpConstrained = oneUpMedia.matches;
      fullModeAvailable = fullMedia.matches;
      if (oneUpMedia.matches && compactView === 'preview' && !selected && !regexTesterOpen) {
        compactView = 'results';
      }
    };

    syncConstraint();
    oneUpMedia.addEventListener('change', syncConstraint);
    fullMedia.addEventListener('change', syncConstraint);

    recentSearches = loadCriteria(RECENT_SEARCHES_KEY);
    savedSearches = loadCriteria(SAVED_SEARCHES_KEY);
    void checkForAvailableUpdate();
    void listen('open-improve-searchmonkey', () => {
      openTelemetryPreferences();
    }).then((unlisten) => {
      improveMenuEventUnlisten = unlisten;
    });
    void listen('open-about-searchmonkey', () => {
      aboutDialogOpen = true;
    }).then((unlisten) => {
      aboutMenuEventUnlisten = unlisten;
    });
    void listen('open-release-notes', () => {
      void openUrl(RELEASE_NOTES_URL).catch(() => {});
    }).then((unlisten) => {
      releaseNotesMenuEventUnlisten = unlisten;
    });
    void listen('open-searchmonkey-website', () => {
      void openUrl(WEBSITE_URL).catch(() => {});
    }).then((unlisten) => {
      websiteMenuEventUnlisten = unlisten;
    });
    void listen('open-report-issue', () => {
      void openUrl(REPORT_ISSUE_URL).catch(() => {});
    }).then((unlisten) => {
      reportIssueMenuEventUnlisten = unlisten;
    });
    void listen('check-for-updates', () => {
      void checkForAvailableUpdate(true);
    }).then((unlisten) => {
      checkForUpdatesMenuEventUnlisten = unlisten;
    });
    telemetryState = loadTelemetryState();
    if (!telemetryState.prompted || !telemetryState.consent) {
      telemetryFirstRun = true;
      telemetryDialogOpen = true;
    } else {
      void syncTelemetryConsent(telemetryState).then((nextState) => {
        telemetryState = nextState;
        if (hasPendingTelemetrySync(nextState)) {
          telemetryFirstRun = false;
          telemetryDialogOpen = true;
        }
      });
    }

    homeDir()
      .then((home) => {
        defaultHomePath = home;
        if (!path) path = home;
      })
      .catch(() => {
        if (!path) path = '/';
      });

    return () => {
      oneUpMedia.removeEventListener('change', syncConstraint);
      fullMedia.removeEventListener('change', syncConstraint);
      clearStatusPollTimer();
      clearElapsedTimer();
      clearResultFlushTimer();
      improveMenuEventUnlisten?.();
      improveMenuEventUnlisten = null;
      aboutMenuEventUnlisten?.();
      aboutMenuEventUnlisten = null;
      releaseNotesMenuEventUnlisten?.();
      releaseNotesMenuEventUnlisten = null;
      websiteMenuEventUnlisten?.();
      websiteMenuEventUnlisten = null;
      reportIssueMenuEventUnlisten?.();
      reportIssueMenuEventUnlisten = null;
      checkForUpdatesMenuEventUnlisten?.();
      checkForUpdatesMenuEventUnlisten = null;
      void cleanupSearchListeners();
    };
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
    const compareText = (left: string, right: string) =>
      left.localeCompare(right, undefined, { numeric: true, sensitivity: 'base' });

    if (sortBy === 'file_name') {
      return nextGroups.sort((a, b) => multiplier * compareText(filename(a.path), filename(b.path)));
    }

    if (sortBy === 'path') {
      return nextGroups.sort(
        (a, b) =>
          multiplier * compareText(parentPath(a.path), parentPath(b.path)) ||
          multiplier * compareText(filename(a.path), filename(b.path))
      );
    }

    if (sortBy === 'modified_date') {
      return nextGroups.sort(
        (a, b) => multiplier * ((a.matches[0]?.modified_secs ?? 0) - (b.matches[0]?.modified_secs ?? 0))
      );
    }

    if (sortBy === 'match_count') {
      return nextGroups.sort(
        (a, b) => multiplier * (a.matches.length - b.matches.length) || compareText(a.path, b.path)
      );
    }

    if (sortBy === 'file_size') {
      return nextGroups.sort(
        (a, b) => multiplier * ((a.matches[0]?.file_size ?? 0) - (b.matches[0]?.file_size ?? 0)) || a.path.localeCompare(b.path)
      );
    }

    return nextGroups;
  }

  function formatCount(count: number) {
    return new Intl.NumberFormat().format(count);
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
    if (pattern.includes('*')) return [normalizeGlobPattern(pattern)];

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
      includePatterns: settings.includeFilters ? [...includePatterns] : [],
      excludePatterns: settings.includeFilters ? [...excludePatterns] : [],
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
    includePatterns = [...criteria.includePatterns];
    excludePatterns = [...criteria.excludePatterns];
    options = { ...defaultSearchOptions(), ...criteria.options };
    contextLines = options.context_lines;
  }

  function normalizedPathForCompare(filePath: string) {
    return filePath.trim().replace(/[/\\]+$/, '');
  }

  function truncatePresetName(name: string) {
    if (name.length <= 48) return name;
    return `${name.slice(0, 45).trim()}...`;
  }

  function suggestedPresetName() {
    const cleanQuery = query.trim().replace(/\s+/g, ' ');
    if (cleanQuery) {
      const queryName = options.search_mode === 'regex' ? `${cleanQuery} regex` : cleanQuery;
      return truncatePresetName(queryName);
    }

    const cleanPath = path.trim();
    const cleanHome = normalizedPathForCompare(defaultHomePath);
    if (cleanPath && normalizedPathForCompare(cleanPath) !== cleanHome) {
      return `${filename(cleanPath)} search`;
    }

    return 'My Search';
  }

  function openSaveDialog() {
    saveSearchName = suggestedPresetName();
    saveIncludeFilters = true;
    saveIncludePath = true;
    saveIncludeOptions = true;
    saveDialogOpen = true;
  }

  function saveCurrentCriteria() {
    const cleanName = saveSearchName.trim() || suggestedPresetName();
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
    const nextName = window.prompt('Rename preset', criteria.name)?.trim();
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
          !sameStringArray(search.includePatterns, criteria.includePatterns) ||
          !sameStringArray(search.excludePatterns, criteria.excludePatterns)
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
        includePatterns: Array.isArray(criteria.includePatterns) ? criteria.includePatterns : [],
        excludePatterns: Array.isArray(criteria.excludePatterns) ? criteria.excludePatterns : [],
        options: { ...defaultSearchOptions(), ...criteria.options }
      }));
    } catch {
      return [];
    }
  }

  function saveCriteria(key: string, criteria: SearchCriteria[]) {
    localStorage.setItem(key, JSON.stringify(criteria));
  }

  function openTelemetryPreferences() {
    telemetryFirstRun = false;
    telemetryDialogOpen = true;
  }

  function closeTelemetryPreferences() {
    if (telemetryFirstRun) return;
    telemetryDialogOpen = false;
  }

  function closeAboutDialog() {
    aboutDialogOpen = false;
  }

  function handleTelemetrySaved(nextState: TelemetryState) {
    telemetryState = nextState;
    telemetryFirstRun = false;
    telemetryDialogOpen = false;
  }

  function hasPendingTelemetrySync(state: TelemetryState) {
    return Boolean(state.consent && state.lastSubmittedConsent !== state.consent);
  }

  function sameStringArray(a: string[], b: string[]) {
    return a.length === b.length && a.every((value, index) => value === b[index]);
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

  function backendStateToUiState(state: SearchStatusChangedEvent['state']): SearchState {
    if (state === 'Starting') return 'starting';
    if (state === 'Running') return 'running';
    if (state === 'Cancelling') return 'cancelling';
    if (state === 'Completed') return 'completed';
    if (state === 'Cancelled') return 'cancelled';
    return 'failed';
  }

  function isSearchActive(state: SearchState) {
    return state === 'starting' || state === 'running' || state === 'cancelling';
  }

  function isTerminalBackendState(state: SearchStatusChangedEvent['state']) {
    return state === 'Completed' || state === 'Cancelled' || state === 'Failed';
  }

  function startStatusPolling(searchId: number) {
    clearStatusPollTimer();
    void refreshSearchStatus(searchId);
    searchStatusPollTimer = setInterval(() => {
      void refreshSearchStatus(searchId);
    }, SEARCH_STATUS_POLL_MS);
  }

  async function refreshSearchStatus(searchId: number) {
    if (searchId !== activeSearchId || statusPollInFlight) return;

    statusPollInFlight = true;
    try {
      const status = await getSearchStatus(searchId);
      if (searchId !== activeSearchId) return;

      backendMatchCount = status.total_matches;
      searchState = backendStateToUiState(status.state);
      if (status.error_message) errorMessage = status.error_message;
      await pullResults(searchId, isTerminalBackendState(status.state));

      if (isTerminalBackendState(status.state)) {
        await finishSearch(searchId, status.state, status.total_matches, status.error_message);
      }
    } catch (error) {
      if (searchId !== activeSearchId) return;
      errorMessage = normalizeError(error);
      searchState = 'failed';
      clearStatusPollTimer();
      stopElapsedTimer();
    } finally {
      statusPollInFlight = false;
    }
  }

  async function finishSearch(
    searchId: number,
    state: SearchStatusChangedEvent['state'],
    totalMatches: number,
    statusError: string | null = null
  ) {
    if (searchId !== activeSearchId || finishingSearchId === searchId) return;

    finishingSearchId = searchId;
    clearStatusPollTimer();
    searchState = backendStateToUiState(state);
    backendMatchCount = totalMatches;
    if (statusError) errorMessage = statusError;
    await pullResults(searchId, true);
    flushQueuedMatches();
    console.info(
      `[searchmonkey] search ${searchId} ${state}: backend_total=${backendMatchCount}, displayed=${matches.length}`
    );
    activeSearchId = null;
    finishingSearchId = null;
    void cleanupSearchListeners();
    stopElapsedTimer();
  }

  async function pullResults(searchId: number, immediateRender = false) {
    if (searchId !== activeSearchId) return;
    if (resultPullPromise) {
      pendingResultPull = true;
      await resultPullPromise;
      if (searchId === activeSearchId && pendingResultPull) {
        await pullResults(searchId, immediateRender);
      }
      return;
    }

    resultPullPromise = drainResults(searchId, immediateRender);
    try {
      await resultPullPromise;
    } catch (error) {
      if (searchId !== activeSearchId) return;
      errorMessage = normalizeError(error);
      searchState = 'failed';
      stopElapsedTimer();
    } finally {
      resultPullPromise = null;
    }
  }

  async function drainResults(searchId: number, immediateRender: boolean): Promise<void> {
    do {
      pendingResultPull = false;
      const nextMatches = await getResults(searchId, matches.length, 1000);
      if (searchId !== activeSearchId) return;
      appendMatches(nextMatches, immediateRender);
      if (nextMatches.length < 1000 || matches.length >= MAX_DISPLAYED_MATCHES) break;
    } while (true);

    if (pendingResultPull && searchId === activeSearchId && matches.length < MAX_DISPLAYED_MATCHES) {
      await drainResults(searchId, immediateRender);
    }
  }

  function handleSearchBufferUpdated(event: SearchBufferUpdatedEvent) {
    if (event.search_id !== activeSearchId) {
      console.warn(`[searchmonkey] ignored buffer update for search ${event.search_id}; active=${activeSearchId}`);
      return;
    }

    backendMatchCount = event.total_matches;
    void pullResults(event.search_id);
  }

  async function handleSearchStatusChanged(event: SearchStatusChangedEvent) {
    if (event.search_id !== activeSearchId) {
      console.warn(`[searchmonkey] ignored status for search ${event.search_id}; active=${activeSearchId}`);
      return;
    }

    searchState = backendStateToUiState(event.state);

    if (isTerminalBackendState(event.state)) {
      try {
        const status = await getSearchStatus(event.search_id);
        if (event.search_id !== activeSearchId) return;
        await finishSearch(event.search_id, status.state, status.total_matches, status.error_message);
      } catch (error) {
        if (event.search_id !== activeSearchId) return;
        errorMessage = normalizeError(error);
        await finishSearch(event.search_id, event.state, backendMatchCount);
      }
    }
  }

  async function startSearch() {
    if (isSearchActive(searchState)) return;
    await cleanupSearchListeners();

    const cleanQuery = query.trim();
    const cleanPath = path.trim();

    if (!cleanQuery) {
      searchState = 'failed';
      errorMessage = 'Enter search text before starting.';
      return;
    }

    if (!cleanPath) {
      searchState = 'failed';
      errorMessage = 'Choose a folder or file path before starting.';
      return;
    }

    searchState = 'starting';
    errorMessage = '';
    elapsedMs = 0;
    backendMatchCount = 0;
    resultPullPromise = null;
    pendingResultPull = false;
    statusPollInFlight = false;
    finishingSearchId = null;
    clearStatusPollTimer();
    startElapsedTimer();
    hasSearched = true;
    selected = null;
    resetMatches();
    clearResultFlushTimer();
    rememberRecentSearch();

    try {
      searchUnlisteners = [
        await listenSearchBufferUpdated(handleSearchBufferUpdated),
        await listenSearchStatusChanged(handleSearchStatusChanged)
      ];
      const includeFromType = includePatternsForType();
      const searchId = await startSearchCommand(
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
        }
      );
      activeSearchId = searchId;
      const status = await getSearchStatus(searchId);
      backendMatchCount = status.total_matches;
      searchState = backendStateToUiState(status.state);
      await pullResults(searchId, isTerminalBackendState(status.state));
      if (isTerminalBackendState(status.state)) {
        await finishSearch(searchId, status.state, status.total_matches, status.error_message);
      } else {
        startStatusPolling(searchId);
      }
    } catch (error) {
      resetMatches();
      clearResultFlushTimer();
      clearStatusPollTimer();
      selected = null;
      activeSearchId = null;
      await cleanupSearchListeners();
      searchState = 'failed';
      stopElapsedTimer();
      errorMessage = normalizeError(error);
    }
  }

  async function cancelSearch() {
    if (activeSearchId === null || !isSearchActive(searchState)) return;

    searchState = 'cancelling';
    try {
      await cancelSearchCommand(activeSearchId);
    } catch (error) {
      errorMessage = normalizeError(error);
      searchState = 'failed';
      stopElapsedTimer();
    }
  }

  async function cleanupSearchListeners() {
    for (const unlisten of searchUnlisteners) {
      unlisten();
    }
    searchUnlisteners = [];
  }

  function handleGlobalKeydown(event: KeyboardEvent) {
    if ((event.metaKey || event.ctrlKey) && !event.shiftKey && ['1', '2', '3'].includes(event.key)) {
      event.preventDefault();
      setLayoutMode(event.key === '1' ? 'focus' : event.key === '2' ? 'split' : 'full');
      return;
    }

    if ((event.metaKey || event.ctrlKey) && event.shiftKey && event.key.toLowerCase() === 'r' && searchModeRegex()) {
      event.preventDefault();
      if (regexTesterOpen) {
        closeRegexTester();
      } else {
        openRegexTester();
      }
      return;
    }

    if (event.key === 'Escape' && compactView === 'preview') {
      event.preventDefault();
      closePreview();
      return;
    }

    if (isEditableTarget(event.target)) return;

    if ((event.key === 'Enter' || event.key === 'F4') && selected && (compactView === 'preview' || activeLayoutMode !== 'focus')) {
      event.preventDefault();
      selectFileMatchOffset(event.shiftKey ? -1 : 1);
      return;
    }

    if (event.key === 'Enter' && selected && compactView === 'results') {
      event.preventDefault();
      compactView = 'preview';
      return;
    }

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

  function openerPath(filePath: string) {
    const normalizedPath = filePath.trim();
    if (!normalizedPath) return '';
    if (/^(?:[a-zA-Z]:[\\/]|\/|\\\\)/.test(normalizedPath)) return normalizedPath;

    return `${path.replace(/[\\/]+$/, '')}/${normalizedPath.replace(/^[\\/]+/, '')}`;
  }

  async function openFile(filePath: string) {
    const targetPath = openerPath(filePath);
    if (!targetPath) return;

    try {
      await openFilePath(targetPath);
    } catch (error) {
      errorMessage = normalizeError(error);
    }
  }

  async function revealFile(filePath: string) {
    const targetPath = openerPath(filePath);
    if (!targetPath) return;

    try {
      await revealFilePath(targetPath);
    } catch (error) {
      errorMessage = normalizeError(error);
    }
  }

  function selectOffset(offset: number) {
    if (!displayedMatches.length) return;

    const currentIndex = selectedIndex >= 0 ? selectedIndex : 0;
    const nextIndex = (currentIndex + offset + displayedMatches.length) % displayedMatches.length;
    const nextMatch = displayedMatches[nextIndex];
    scheduleResultFlush(SEARCH_RESULT_FLUSH_WHILE_PREVIEW_LOADING_MS);
    previewViewport = updateViewportForMatch(nextMatch);
    selected = nextMatch;
  }

  function selectFileOffset(offset: number, targetMatch: 'first' | 'last' = 'first') {
    if (!groups.length) return;

    const currentPath = selected?.path;
    const currentGroupIndex = currentPath ? groups.findIndex((group) => group.path === currentPath) : -1;
    const startIndex = currentGroupIndex >= 0 ? currentGroupIndex : 0;
    const nextGroupIndex = (startIndex + offset + groups.length) % groups.length;
    const nextGroup = groups[nextGroupIndex];
    const nextMatch = targetMatch === 'last' ? nextGroup?.matches.at(-1) : nextGroup?.matches[0];
    if (!nextMatch) return;

    scheduleResultFlush(SEARCH_RESULT_FLUSH_WHILE_PREVIEW_LOADING_MS);
    previewViewport = updateViewportForMatch(nextMatch);
    selected = nextMatch;
  }

  function selectFileMatchOffset(offset: number) {
    if (!selected) return;

    const current = selected;
    const group = groups.find((resultGroup) => resultGroup.path === current.path);
    if (!group?.matches.length) return;

    const currentIndex = selectedFileMatchIndex >= 0 ? selectedFileMatchIndex : 0;
    if (offset > 0 && currentIndex === group.matches.length - 1) {
      selectFileOffset(1);
      return;
    }

    if (offset < 0 && currentIndex === 0) {
      selectFileOffset(-1, 'last');
      return;
    }

    const nextIndex = currentIndex + offset;
    const nextMatch = group.matches[nextIndex];

    scheduleResultFlush(SEARCH_RESULT_FLUSH_WHILE_PREVIEW_LOADING_MS);
    previewViewport = updateViewportForMatch(nextMatch);
    selected = nextMatch;
  }

  function scheduleResultFlush(delay = previewIsLoading ? SEARCH_RESULT_FLUSH_WHILE_PREVIEW_LOADING_MS : SEARCH_RESULT_FLUSH_MS) {
    if (!pendingMatchesRender) return;

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
    clearResultFlushTimer();
    renderPendingMatches();
  }

  function clearResultFlushTimer() {
    if (!resultFlushTimer) return;

    clearTimeout(resultFlushTimer);
    resultFlushTimer = null;
  }

  function clearStatusPollTimer() {
    if (!searchStatusPollTimer) return;

    clearInterval(searchStatusPollTimer);
    searchStatusPollTimer = null;
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
    const scopeWidth = layoutMode === 'full' && scopePanelVisible ? 280 : 0;
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
    if (activeLayoutMode === 'focus') {
      layoutMode = 'split';
    }
    regexTesterOpen = true;
    compactView = 'preview';
  }

  function closeRegexTester() {
    regexTesterOpen = false;
    compactView = 'results';
  }

  function toggleRegexTester() {
    if (regexTesterOpen) {
      closeRegexTester();
    } else {
      openRegexTester();
    }
  }

  function setLayoutMode(mode: 'focus' | 'split' | 'full') {
    if (oneUpConstrained && mode !== 'focus') {
      return;
    }

    if (mode === 'full' && !fullModeAvailable) {
      return;
    }

    layoutMode = mode;
    if (mode === 'focus') {
      compactView = 'results';
      regexTesterOpen = false;
    }
  }

  $effect(() => {
    if (oneUpConstrained && compactView === 'preview' && !selected && !regexTesterOpen) {
      compactView = 'results';
    }
  });

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

<main class="app-shell" class:full-layout={activeLayoutMode === 'full'} class:has-update={Boolean(availableUpdate)}>
  <SearchBar
    bind:query
    bind:options
    searching={isSearchActive(searchState)}
    {savedSearches}
    layoutMode={activeLayoutMode}
    availableLayoutModes={[...availableLayoutModes]}
    onFilters={scopePanelVisibleInLayout ? undefined : () => (filtersOpen = true)}
    onLayoutMode={setLayoutMode}
    onRegexTester={toggleRegexTester}
    onApplyCriteria={applyCriteria}
    onSaveRequest={openSaveDialog}
    onRenameCriteria={renameSavedSearch}
    onDeleteCriteria={deleteSavedSearch}
    onSearch={startSearch}
    onCancel={cancelSearch}
  />

  <div class="scope-summary" aria-label="Search scope">
    <div class="scope-path">
      <PathInput
        id="search-path-inline"
        bind:value={path}
        placeholder="/Users/name/project"
        includeHidden={options.hidden}
      />
    </div>
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

  {#if availableUpdate}
    <section class="update-cta" aria-live="polite" aria-label="Searchmonkey update available">
      <div class="update-copy">
        <strong>Searchmonkey {availableUpdate.tagName} is available.</strong>
        <span>You are running {availableUpdate.currentVersion}.</span>
      </div>
      <div class="update-actions">
        <button class="update-primary" type="button" title={availableUpdate.downloadName} onclick={openUpdateDownload}>
          Download update
        </button>
        <button class="update-secondary" type="button" onclick={openUpdateReleaseNotes}>
          Release notes
        </button>
        <button class="update-dismiss" type="button" aria-label={`Dismiss update ${availableUpdate.tagName}`} onclick={dismissUpdate}>
          x
        </button>
      </div>
    </section>
  {/if}

  <div class="results-toolbar" aria-label="Results actions">
    <span>{groups.length} files</span>
    <span>{displayedMatchCount} matches</span>
    <button type="button" onclick={() => (filtersOpen = true)}>Filters</button>
    <button type="button" disabled={!selected} onclick={() => (compactView = 'preview')}>Preview</button>
  </div>

  <div
    bind:this={workspaceElement}
    class:resizing={isResizingPreview}
    class:has-preview={Boolean(selected) || regexTesterOpen}
    class:show-preview={compactView === 'preview' || regexTesterOpen}
    class:layout-focus={activeLayoutMode === 'focus'}
    class:layout-split={activeLayoutMode === 'split'}
    class:layout-full={activeLayoutMode === 'full'}
    class="workspace"
    style:--preview-width={`${previewWidth}px`}
    style:grid-template-columns={workspaceGridTemplate}
  >
    <ScopePanel
      bind:includePatterns
      bind:excludePatterns
      bind:contextLines
      bind:options
    />
    <ResultsPanel
      {groups}
      {query}
      searchPath={path}
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
        activeFileMatchNumber={selectedFileMatchIndex + 1}
        activeFileMatchTotal={selectedFileMatchCount}
        canNavigateFiles={groups.length > 1}
        drilldown={activeLayoutMode === 'focus'}
        onPrevious={() => selectFileMatchOffset(-1)}
        onNext={() => selectFileMatchOffset(1)}
        onPreviousFile={() => selectFileOffset(-1)}
        onNextFile={() => selectFileOffset(1)}
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
          bind:includePatterns
          bind:excludePatterns
          bind:contextLines
          bind:options
        />
      </div>
    </div>
  {/if}

  {#if saveDialogOpen}
    <div class="modal-layer" role="presentation">
      <button
        class="modal-backdrop"
        type="button"
        aria-label="Close save preset"
        onclick={() => (saveDialogOpen = false)}
      ></button>
      <div class="save-dialog" role="dialog" aria-modal="true" aria-label="Save preset">
        <form onsubmit={(event) => { event.preventDefault(); saveCurrentCriteria(); }}>
          <header>
            <h2>Save Preset</h2>
          </header>
          <div class="save-body">
            <div class="field">
              <label for="saved-search-name">Name</label>
              <input id="saved-search-name" type="text" bind:value={saveSearchName} placeholder="Name" autocomplete="off" />
            </div>
            <div class="remember-group" aria-labelledby="save-preset-remember">
              <p id="save-preset-remember">This preset will remember:</p>
              <label class="check-row">
                <input type="checkbox" bind:checked={saveIncludePath} />
                <span>Search location</span>
              </label>
              <label class="check-row">
                <input type="checkbox" bind:checked={saveIncludeFilters} />
                <span>Filters and regex</span>
              </label>
              <label class="check-row">
                <input type="checkbox" bind:checked={saveIncludeOptions} />
                <span>Layout and sorting</span>
              </label>
            </div>
          </div>
          <footer>
            <button type="button" onclick={() => (saveDialogOpen = false)}>Cancel</button>
            <button class="primary-save" type="submit">Save</button>
          </footer>
        </form>
      </div>
    </div>
  {/if}

  {#if telemetryDialogOpen && telemetryState}
    <TelemetryConsentDialog
      firstRun={telemetryFirstRun}
      telemetry={telemetryState}
      onClose={closeTelemetryPreferences}
      onSaved={handleTelemetrySaved}
    />
  {/if}

  {#if aboutDialogOpen}
    <AboutDialog onClose={closeAboutDialog} />
  {/if}

  <StatusBar
    state={searchState}
    totalMatches={Math.max(displayedMatchCount, backendMatchCount)}
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
    --surface: #f7f9fa;
    --panel: #ffffff;
    --input: #ffffff;
    --disabled: #edf0f3;
    --border: #d9dee5;
    --border-subtle: #e7ebef;
    --border-strong: #c5ccd5;
    --accent: #16834a;
    --accent-strong: #0b5f32;
    --accent-soft: #79bf94;
    --accent-wash: #e7f5ec;
    --focus: rgba(22, 131, 74, 0.18);
    --selection: #f1f6f3;
    --selection-strong: #e2eee7;
    --preview-bg: #eaf0ee;
    --code-bg: #fbfcfb;
    --highlight: rgba(229, 174, 56, 0.36);
    --highlight-strong: rgba(220, 143, 26, 0.54);
    --highlight-row: rgba(229, 174, 56, 0.18);
    --highlight-row-soft: rgba(229, 174, 56, 0.08);
    --ok: var(--accent);
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
    grid-template-rows: auto auto minmax(0, 1fr) auto;
    width: 100vw;
    height: 100vh;
    background: var(--surface);
  }

  .app-shell.full-layout {
    grid-template-rows: auto auto minmax(0, 1fr) auto;
  }

  .app-shell.has-update,
  .app-shell.full-layout.has-update {
    grid-template-rows: auto auto auto minmax(0, 1fr) auto;
  }

  .results-toolbar {
    display: none;
  }

  .scope-summary {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 8px;
    align-items: end;
    min-height: 30px;
    border-bottom: 1px solid var(--border);
    padding: 6px 12px;
    color: var(--muted);
    background: var(--panel);
    font-size: 12px;
    font-weight: 650;
  }

  .update-cta {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 10px;
    align-items: center;
    border-bottom: 1px solid #c8d6cc;
    padding: 8px 12px;
    background: #ecf7ef;
    color: var(--text);
    font-size: 12px;
  }

  .update-copy {
    display: flex;
    min-width: 0;
    flex-wrap: wrap;
    gap: 5px 8px;
    align-items: center;
  }

  .update-copy strong {
    color: var(--accent-strong);
  }

  .update-copy span {
    color: #54616a;
    font-weight: 650;
  }

  .update-actions {
    display: flex;
    gap: 6px;
    align-items: center;
    justify-content: flex-end;
  }

  .update-actions button {
    height: 30px;
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0 9px;
    font: inherit;
    font-size: 12px;
    font-weight: 800;
    cursor: pointer;
  }

  .update-primary {
    border-color: var(--accent) !important;
    color: #ffffff;
    background: var(--accent);
  }

  .update-primary:hover,
  .update-primary:focus-visible {
    background: var(--accent-strong);
    outline: none;
  }

  .update-secondary,
  .update-dismiss {
    color: var(--accent-strong);
    background: #ffffff;
  }

  .update-secondary:hover,
  .update-secondary:focus-visible,
  .update-dismiss:hover,
  .update-dismiss:focus-visible {
    border-color: var(--accent-soft);
    background: var(--accent-wash);
    outline: none;
  }

  .update-dismiss {
    width: 30px;
    padding: 0;
  }

  .scope-path {
    min-width: 0;
  }

  .scope-summary :global(.path-control) {
    gap: 3px;
  }

  .scope-summary :global(.breadcrumbs) {
    display: none;
  }

  .mode-pills {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
    justify-content: flex-end;
  }

  .mode-pills button {
    height: 32px;
    border: 1px solid var(--border-subtle);
    border-radius: 999px;
    padding: 0 10px;
    color: var(--muted);
    background: var(--surface);
    font: inherit;
    font-size: 12px;
    font-weight: 800;
  }

  .mode-pills button.active {
    border-color: var(--accent-soft);
    color: var(--accent-strong);
    background: var(--accent-wash);
  }

  .workspace {
    display: grid;
    min-height: 0;
  }

  .workspace.layout-split > :global(.scope-panel),
  .workspace.layout-focus > :global(.scope-panel),
  .workspace.layout-focus > .panel-resizer {
    display: none;
  }

  .workspace.layout-focus {
    grid-template-columns: minmax(0, 1fr) !important;
    grid-template-rows: minmax(0, 1fr);
  }

  .workspace.layout-focus > :global(.results-panel),
  .workspace.layout-focus > :global(.preview-panel),
  .workspace.layout-focus > :global(.regex-panel) {
    grid-column: 1;
    grid-row: 1;
    min-height: 0;
  }

  .workspace.layout-focus > :global(.preview-panel),
  .workspace.layout-focus > :global(.regex-panel),
  .workspace.layout-focus.show-preview > :global(.results-panel) {
    display: none;
  }

  .workspace.layout-focus.show-preview > :global(.preview-panel),
  .workspace.layout-focus.show-preview > :global(.regex-panel) {
    display: grid;
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
    background: var(--preview-bg);
    cursor: col-resize;
  }

  .panel-resizer:hover,
  .panel-resizer:focus-visible {
    background: var(--accent-wash);
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
    width: 100vw;
    max-width: 100vw;
    z-index: 35;
    display: grid;
    align-items: center;
    justify-items: center;
    padding: 18px;
    overflow: auto;
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
    width: min(340px, 100%);
    max-width: calc(100vw - 24px);
    margin-inline: auto;
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
    background: var(--panel);
    box-shadow: 0 18px 42px rgba(30, 37, 45, 0.22);
  }

  .save-dialog form {
    display: grid;
    grid-template-rows: auto auto auto;
  }

  .save-dialog header,
  .save-dialog footer {
    display: flex;
    align-items: center;
    justify-content: flex-start;
    gap: 10px;
    padding: 12px 14px 8px;
    background: var(--panel);
  }

  .save-dialog footer {
    justify-content: flex-end;
    gap: 8px;
    padding: 4px 14px 14px;
  }

  .save-dialog h2 {
    margin: 0;
    font-size: 15px;
    line-height: 1.3;
  }

  .save-body {
    display: grid;
    gap: 14px;
    padding: 8px 14px 12px;
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

  .save-body .field > label {
    color: var(--text);
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
    gap: 9px;
    align-items: center;
  }

  .save-body .remember-group {
    display: grid;
    gap: 10px;
  }

  .save-body .remember-group p {
    margin: 0 0 2px;
    color: var(--text);
    font-size: 12px;
    font-weight: 750;
  }

  .save-body .remember-group input[type='checkbox'] {
    margin: 0;
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
    .scope-summary {
      min-height: 30px;
    }
  }

  @media (max-width: 849px) {
    .workspace {
      grid-template-columns: minmax(0, 1fr) !important;
      grid-template-rows: minmax(0, 1fr);
    }

    .workspace > :global(.scope-panel) {
      display: none;
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
      grid-template-columns: minmax(0, 1fr) auto;
    }

    .update-cta {
      grid-template-columns: minmax(0, 1fr);
      gap: 8px;
    }

    .update-actions {
      justify-content: flex-start;
    }
  }

  @media (max-width: 599px) {
    .app-shell {
      grid-template-rows: auto auto minmax(0, 1fr) auto;
    }

    .app-shell.full-layout {
      grid-template-rows: auto minmax(0, 1fr) auto;
    }

    .app-shell.has-update {
      grid-template-rows: auto auto auto minmax(0, 1fr) auto;
    }

    .app-shell.full-layout.has-update {
      grid-template-rows: auto auto auto minmax(0, 1fr) auto;
    }

    .scope-summary {
      grid-template-columns: minmax(0, 1fr);
      min-height: 28px;
      padding: 6px 8px;
    }

    .mode-pills {
      justify-content: flex-start;
    }

    .results-toolbar {
      display: none;
    }
  }
</style>
