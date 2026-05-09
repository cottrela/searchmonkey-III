<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { filename, parentPath } from '$lib/paths';
  import { copyText } from '$lib/clipboard';
  import { defaultSearchOptions, type FileResultGroup, type SearchMatch, type SearchOptions, type SearchState } from '$lib/types';

  type SnippetPart = {
    text: string;
    hit: boolean;
  };

  type ResultRow =
    | {
        type: 'file';
        key: string;
        path: string;
        count: number;
        top: number;
        height: number;
      }
    | {
        type: 'match';
        key: string;
        match: SearchMatch;
        top: number;
        height: number;
      };

  type SortMode =
    | 'relevance_desc'
    | 'file_name_asc'
    | 'file_name_desc'
    | 'path_asc'
    | 'path_desc'
    | 'match_count_desc'
    | 'match_count_asc';

  type ScrollLandmark = {
    cue: string;
    primary: string;
    secondary: string;
  };

  const FULL_LINE_LIMIT = 200;
  const SNIPPET_CONTEXT = 64;
  const FILE_ROW_HEIGHT = 44;
  const MATCH_ROW_HEIGHT = 28;
  const OVERSCAN = 12;
  const MOBILE_MATCH_LIMIT = 10;
  const SORT_MODE_OPTIONS: Array<{ value: SortMode; label: string }> = [
    { value: 'relevance_desc', label: 'Default' },
    { value: 'file_name_asc', label: 'File name A-Z' },
    { value: 'file_name_desc', label: 'File name Z-A' },
    { value: 'path_asc', label: 'Path A-Z' },
    { value: 'path_desc', label: 'Path Z-A' },
    { value: 'match_count_desc', label: 'Most matches' },
    { value: 'match_count_asc', label: 'Fewest matches' }
  ];
  const SUPPORTED_SORT_MODES = new Set(SORT_MODE_OPTIONS.map((option) => option.value));

  let {
    groups,
    query,
    searchPath,
    regex,
    options = $bindable<SearchOptions>(defaultSearchOptions()),
    selected,
    state: searchState,
    hasSearched,
    onSelect,
    onOpen,
    onReveal
  }: {
    groups: FileResultGroup[];
    query: string;
    searchPath: string;
    regex: boolean;
    options: SearchOptions;
    selected: SearchMatch | null;
    state: SearchState;
    hasSearched: boolean;
    onSelect: (match: SearchMatch) => void;
    onOpen: (path: string) => void;
    onReveal: (path: string) => void;
  } = $props();

  let resultsElement = $state<HTMLElement | undefined>();
  let lastScrolledMatch = '';
  let scrollTop = $state(0);
  let viewportHeight = $state(0);
  let scrollScrubbing = $state(false);
  let scrubPreviewTop = $state(0);
  let scrubPreviewLeft = $state(0);
  let expandedFiles = $state<Set<string>>(new Set());

  const rows = $derived.by(() => buildRows(groups));
  const matchTotal = $derived.by(() => groups.reduce((total, group) => total + group.matches.length, 0));
  const currentSortMode = $derived.by(() => {
    const mode = `${options.sort_by}_${options.sort_direction}` as SortMode;
    return SUPPORTED_SORT_MODES.has(mode) ? mode : 'relevance_desc';
  });
  const totalHeight = $derived.by(() => {
    const last = rows.at(-1);
    return last ? last.top + last.height : 0;
  });
  const visibleRows = $derived.by(() => {
    const start = Math.max(0, scrollTop - OVERSCAN * MATCH_ROW_HEIGHT);
    const end = scrollTop + viewportHeight + OVERSCAN * MATCH_ROW_HEIGHT;

    return rows.filter((row) => row.top + row.height >= start && row.top <= end);
  });
  const currentFileRow = $derived.by(() => {
    let current: Extract<ResultRow, { type: 'file' }> | null = null;

    for (const row of rows) {
      if (row.top > scrollTop) break;
      if (row.type === 'file') current = row;
    }

    return current;
  });
  const currentLandmark = $derived.by(() => landmarkForScrollPosition(scrollTop + Math.max(0, viewportHeight * 0.32)));

  onMount(() => {
    if (!resultsElement) return;

    viewportHeight = resultsElement.clientHeight;
    const observer = new ResizeObserver(() => {
      viewportHeight = resultsElement?.clientHeight ?? 0;
    });

    observer.observe(resultsElement);
    return () => observer.disconnect();
  });

  function matchKey(match: SearchMatch) {
    const ranges = match.submatches.map((range) => `${range.start}-${range.end}`).join(',');
    return `${match.path}:${match.line_number}:${ranges}:${match.line_text}`;
  }

  function buildRows(resultGroups: FileResultGroup[]): ResultRow[] {
    const nextRows: ResultRow[] = [];
    let top = 0;

    for (const group of resultGroups) {
      if (options.group_by_file) {
        nextRows.push({
          type: 'file',
          key: `file:${group.path}`,
          path: group.path,
          count: group.matches.length,
          top,
          height: FILE_ROW_HEIGHT
        });
        top += FILE_ROW_HEIGHT;
      }

      for (const [index, match] of group.matches.entries()) {
        nextRows.push({
          type: 'match',
          key: `match:${group.path}:${match.line_number}:${index}`,
          match,
          top,
          height: MATCH_ROW_HEIGHT
        });
        top += MATCH_ROW_HEIGHT;
      }
    }

    return nextRows;
  }

  function sameMatch(a: SearchMatch | null, b: SearchMatch) {
    return Boolean(a && a.path === b.path && a.line_number === b.line_number && a.line_text === b.line_text);
  }

  function snippetParts(match: SearchMatch, term: string): SnippetPart[] {
    const spans = match.submatches?.length ? match.submatches : fallbackSpans(match.line_text, term);
    const snippet = snippetWindow(match.line_text, spans);
    const visibleSpans = spans
      .map((span) => ({
        start: Math.max(span.start, snippet.start) - snippet.start,
        end: Math.min(span.end, snippet.end) - snippet.start
      }))
      .filter((span) => span.start < span.end);

    const parts = splitSnippet(match.line_text.slice(snippet.start, snippet.end), visibleSpans);

    if (snippet.clippedStart) {
      parts.unshift({ text: '...', hit: false });
    }

    if (snippet.clippedEnd) {
      parts.push({ text: '...', hit: false });
    }

    return parts;
  }

  function snippetWindow(text: string, spans: Array<{ start: number; end: number }>) {
    if (text.length <= FULL_LINE_LIMIT || spans.length === 0) {
      return { start: 0, end: text.length, clippedStart: false, clippedEnd: false };
    }

    const anchor = spans[0];
    const start = Math.max(0, anchor.start - SNIPPET_CONTEXT);
    const end = Math.min(text.length, anchor.end + SNIPPET_CONTEXT);

    return {
      start,
      end,
      clippedStart: start > 0,
      clippedEnd: end < text.length
    };
  }

  function splitSnippet(text: string, spans: Array<{ start: number; end: number }>): SnippetPart[] {
    if (!spans.length) return [{ text, hit: false }];

    const parts: SnippetPart[] = [];
    let cursor = 0;

    for (const span of spans) {
      if (span.start > cursor) {
        parts.push({ text: text.slice(cursor, span.start), hit: false });
      }

      parts.push({ text: text.slice(span.start, span.end), hit: true });
      cursor = span.end;
    }

    if (cursor < text.length) {
      parts.push({ text: text.slice(cursor), hit: false });
    }

    return parts.length ? parts : [{ text, hit: false }];
  }

  function fallbackSpans(text: string, term: string) {
    if (regex || !term) return [];

    const lowerText = text.toLowerCase();
    const lowerTerm = term.toLowerCase();
    const spans: Array<{ start: number; end: number }> = [];
    let cursor = 0;
    let index = lowerText.indexOf(lowerTerm);

    while (index !== -1) {
      spans.push({ start: index, end: index + term.length });
      cursor = index + term.length;
      index = lowerText.indexOf(lowerTerm, cursor);
    }

    return spans;
  }

  function handleAction(event: MouseEvent, action: () => void | Promise<void>) {
    event.preventDefault();
    event.stopPropagation();
    void action();
  }

  function handleCopy(event: MouseEvent, text: string) {
    handleAction(event, async () => {
      await copyText(text);
    });
  }

  function copyFilename(event: MouseEvent, filePath: string) {
    handleCopy(event, filename(filePath));
  }

  function visibleMobileMatches(group: FileResultGroup) {
    if (expandedFiles.has(group.path)) return group.matches;
    return group.matches.slice(0, MOBILE_MATCH_LIMIT);
  }

  function setSortMode(event: Event) {
    const select = event.currentTarget;
    if (!(select instanceof HTMLSelectElement)) return;

    const [sortBy, direction] = select.value.replace(/_(asc|desc)$/, '|$1').split('|');
    options.sort_by = sortBy as SearchOptions['sort_by'];
    options.sort_direction = direction as SearchOptions['sort_direction'];
  }

  function toggleExpandedFile(filePath: string) {
    const next = new Set(expandedFiles);

    if (next.has(filePath)) {
      next.delete(filePath);
    } else {
      next.add(filePath);
    }

    expandedFiles = next;
  }

  function matchLabel(count: number) {
    return `${count} ${count === 1 ? 'match' : 'matches'}`;
  }

  function formatCount(count: number) {
    return new Intl.NumberFormat().format(count);
  }

  function updateScrollMetrics() {
    if (!resultsElement) return;

    scrollTop = resultsElement.scrollTop;
    viewportHeight = resultsElement.clientHeight;
    if (scrollScrubbing) updateScrubPreviewPosition();
  }

  function rowAtPosition(position: number) {
    let low = 0;
    let high = rows.length - 1;

    while (low <= high) {
      const middle = Math.floor((low + high) / 2);
      const row = rows[middle];

      if (position < row.top) {
        high = middle - 1;
      } else if (position > row.top + row.height) {
        low = middle + 1;
      } else {
        return row;
      }
    }

    return rows[Math.max(0, Math.min(rows.length - 1, low))];
  }

  function groupForPath(filePath: string) {
    return groups.find((group) => group.path === filePath);
  }

  function firstCue(value: string) {
    return value.trim().match(/[A-Za-z0-9]/)?.[0]?.toUpperCase() ?? '#';
  }

  function normalizePathForCompare(filePath: string) {
    return filePath.replace(/\\/g, '/').replace(/\/+$/, '');
  }

  function relativePathFromSearchRoot(filePath: string) {
    const normalizedFile = normalizePathForCompare(filePath);
    const normalizedBase = normalizePathForCompare(searchPath);

    if (!normalizedBase || normalizedFile === normalizedBase) return filename(filePath);
    if (!normalizedFile.startsWith(`${normalizedBase}/`)) return filePath.replace(/\\/g, '/');

    return normalizedFile.slice(normalizedBase.length + 1);
  }

  function pathLandmark(filePath: string) {
    const relativePath = relativePathFromSearchRoot(filePath);
    const parts = relativePath.split('/').filter(Boolean);
    const folders = parts.slice(0, -1);
    const fileCue = firstCue(filename(filePath));
    const folderCue = folders[0] ? firstCue(folders[0]) : '';

    return {
      cue: `${folderCue}/${fileCue}`,
      primary: folders.length ? folders.join('/') : '/',
      secondary: filename(filePath)
    };
  }

  function landmarkForScrollPosition(position: number): ScrollLandmark | null {
    const row = rowAtPosition(position);
    if (!row) return null;

    const filePath = row.type === 'file' ? row.path : row.match.path;
    const group = groupForPath(filePath);
    const fileName = filename(filePath);
    const parent = parentPath(filePath);
    const matches = group?.matches.length ?? (row.type === 'file' ? row.count : 1);
    const matchText = `${formatCount(matches)} ${matches === 1 ? 'match' : 'matches'}`;

    if (options.sort_by === 'path') {
      return pathLandmark(filePath);
    }

    if (options.sort_by === 'match_count') {
      return {
        cue: formatCount(matches),
        primary: matchText,
        secondary: fileName
      };
    }

    return {
      cue: firstCue(fileName),
      primary: fileName,
      secondary: matchText
    };
  }

  function updateScrubPreviewPosition() {
    if (!resultsElement) return;

    const rect = resultsElement.getBoundingClientRect();
    const scrollable = Math.max(1, resultsElement.scrollHeight - resultsElement.clientHeight);
    const ratio = resultsElement.scrollTop / scrollable;
    const thumbHeight = Math.max(36, (resultsElement.clientHeight / Math.max(1, resultsElement.scrollHeight)) * resultsElement.clientHeight);
    const thumbRange = Math.max(0, resultsElement.clientHeight - thumbHeight);
    const thumbCenter = rect.top + ratio * thumbRange + thumbHeight / 2;

    scrubPreviewTop = Math.round(Math.min(rect.bottom - 74, Math.max(rect.top + 74, thumbCenter)));
    scrubPreviewLeft = Math.round(Math.max(rect.left + 12, rect.right - 252));
  }

  function stopScrollScrub() {
    scrollScrubbing = false;
    window.removeEventListener('pointerup', stopScrollScrub, true);
    window.removeEventListener('pointercancel', stopScrollScrub, true);
    window.removeEventListener('pointermove', handleScrubPointerMove, true);
    window.removeEventListener('mouseup', stopScrollScrub, true);
    document.removeEventListener('mouseup', stopScrollScrub, true);
    window.removeEventListener('blur', stopScrollScrub, true);
    document.removeEventListener('visibilitychange', handleScrubVisibilityChange, true);
  }

  function handleScrubVisibilityChange() {
    if (document.visibilityState !== 'visible') stopScrollScrub();
  }

  function handleScrubPointerMove(event: PointerEvent) {
    if (!resultsElement) return;

    const rect = resultsElement.getBoundingClientRect();
    const horizontalTolerance = 80;
    const verticalTolerance = 24;
    const outsideBounds =
      event.clientX < rect.right - horizontalTolerance ||
      event.clientX > rect.right + horizontalTolerance ||
      event.clientY < rect.top - verticalTolerance ||
      event.clientY > rect.bottom + verticalTolerance;

    if (outsideBounds) stopScrollScrub();
  }

  function startScrollScrub(event: PointerEvent) {
    if (!resultsElement) return;
    if (window.innerWidth < 600) return;

    const rect = resultsElement.getBoundingClientRect();
    const scrollbarGutter = 22;
    if (event.clientX < rect.right - scrollbarGutter) return;

    scrollScrubbing = true;
    updateScrubPreviewPosition();

    window.addEventListener('pointerup', stopScrollScrub, true);
    window.addEventListener('pointercancel', stopScrollScrub, true);
    window.addEventListener('pointermove', handleScrubPointerMove, true);
    window.addEventListener('mouseup', stopScrollScrub, true);
    document.addEventListener('mouseup', stopScrollScrub, true);
    window.addEventListener('blur', stopScrollScrub, true);
    document.addEventListener('visibilitychange', handleScrubVisibilityChange, true);
  }

  function closeMoreActionMenus(except?: HTMLDetailsElement) {
    resultsElement?.querySelectorAll<HTMLDetailsElement>('.more-actions[open]').forEach((menu) => {
      if (menu !== except) {
        menu.open = false;
      }
    });
  }

  function handleMoreActionsToggle(event: Event) {
    const menu = event.currentTarget;
    if (!(menu instanceof HTMLDetailsElement) || !menu.open) return;
    closeMoreActionMenus(menu);
  }

  function handleMoreActionsFocusOut(event: FocusEvent) {
    const menu = event.currentTarget;
    if (!(menu instanceof HTMLDetailsElement)) return;

    setTimeout(() => {
      if (menu.contains(document.activeElement)) return;
      menu.open = false;
    }, 120);
  }

  $effect(() => {
    if (SUPPORTED_SORT_MODES.has(`${options.sort_by}_${options.sort_direction}` as SortMode)) return;

    options.sort_by = 'relevance';
    options.sort_direction = 'desc';
  });

  $effect(() => {
    const handlePointerDown = (event: PointerEvent) => {
      if (!resultsElement) return;
      if (!(event.target instanceof Node)) return;

      const actionMenu = (event.target instanceof Element ? event.target : event.target.parentElement)?.closest('.more-actions');
      if (actionMenu && resultsElement.contains(actionMenu)) return;

      closeMoreActionMenus();
    };

    document.addEventListener('pointerdown', handlePointerDown, true);
    return () => {
      document.removeEventListener('pointerdown', handlePointerDown, true);
    };
  });

  $effect(() => {
    if (!selected || !resultsElement) return;

    const key = matchKey(selected);
    if (key === lastScrolledMatch) return;

    lastScrolledMatch = key;
    const selectedRow = rows.find((row) => row.type === 'match' && sameMatch(selected, row.match));
    if (selectedRow) {
      resultsElement.scrollTop = Math.max(0, selectedRow.top - resultsElement.clientHeight / 2);
      updateScrollMetrics();
    }

    tick().then(() => {
      const selectedRow = resultsElement?.querySelector("[data-selected-match='true']");
      if (selectedRow instanceof HTMLElement) {
        selectedRow.scrollIntoView({ block: 'center' });
      }
    });
  });
</script>

<section bind:this={resultsElement} class="results-panel" aria-label="Search results" onscroll={updateScrollMetrics} onpointerdown={startScrollScrub}>
  <div class="panel-title">
    <div class="title-block">
      <h2>Results</h2>
      <span>{formatCount(groups.length)} files · {formatCount(matchTotal)} matches</span>
    </div>
    <div class="result-controls" aria-label="Result display settings">
      <label>
        <span>Sort</span>
        <select value={currentSortMode} onchange={setSortMode}>
          {#each SORT_MODE_OPTIONS as sortOption}
            <option value={sortOption.value}>{sortOption.label}</option>
          {/each}
        </select>
      </label>
      <label class="toggle-control">
        <input type="checkbox" bind:checked={options.group_by_file} />
        <span>Group by file</span>
      </label>
      <label class="toggle-control">
        <input type="checkbox" bind:checked={options.show_line_numbers} />
        <span>Show line numbers</span>
      </label>
      <details class="result-options-menu more-actions" ontoggle={handleMoreActionsToggle} onfocusout={handleMoreActionsFocusOut}>
        <summary title="Result options" aria-label="Result options">...</summary>
        <div class="menu">
          <label class="toggle-control">
            <input type="checkbox" bind:checked={options.group_by_file} />
            <span>Group by file</span>
          </label>
          <label class="toggle-control">
            <input type="checkbox" bind:checked={options.show_line_numbers} />
            <span>Show line numbers</span>
          </label>
        </div>
      </details>
    </div>
  </div>

  {#if !hasSearched}
    <div class="empty">Choose a folder and search text files</div>
  {:else if (searchState === 'starting' || searchState === 'running' || searchState === 'cancelling') && groups.length === 0}
    <div class="empty active-search">
      <span class="spinner" aria-hidden="true"></span>
      <span>{searchState === 'cancelling' ? 'Cancelling search...' : 'Searching current files...'}</span>
    </div>
  {:else if groups.length === 0}
    <div class="empty">No matches found</div>
  {:else}
    <div class="mobile-groups">
      {#each groups as group (group.path)}
        <section class="mobile-file-group" aria-label={filename(group.path)}>
          {#if options.group_by_file}
            <div class="mobile-file-header">
            <div class="mobile-file-title">
              <strong title={group.path}>{filename(group.path)}</strong>
              <span title={parentPath(group.path)}>{parentPath(group.path)}</span>
            </div>
            <div class="mobile-file-actions">
              <button type="button" onclick={(event) => handleAction(event, () => onOpen(group.path))}>Open</button>
              <details class="more-actions" ontoggle={handleMoreActionsToggle} onfocusout={handleMoreActionsFocusOut}>
                <summary title="More actions" aria-label="More actions">...</summary>
                <div class="menu">
                  <div class="menu-title">{matchLabel(group.matches.length)}</div>
                  <button type="button" onclick={(event) => handleAction(event, () => onOpen(group.path))}>Open</button>
                  <button type="button" onclick={(event) => handleAction(event, () => onReveal(group.path))}>Reveal</button>
                  <button type="button" onclick={(event) => handleCopy(event, group.path)}>Copy path</button>
                  <button type="button" onclick={(event) => copyFilename(event, group.path)}>Copy filename</button>
                  <button type="button" onclick={() => toggleExpandedFile(group.path)}>
                    {expandedFiles.has(group.path) ? 'Collapse file' : 'Show all matches'}
                  </button>
                </div>
              </details>
            </div>
          </div>
          {/if}

          <div class="mobile-matches">
            {#each visibleMobileMatches(group) as match, index (`${match.path}:${match.line_number}:${index}`)}
              <button
                type="button"
                class:selected={sameMatch(selected, match)}
                class:no-lines={!options.show_line_numbers}
                data-selected-match={sameMatch(selected, match) ? 'true' : undefined}
                class="match-row mobile-match-row"
                onclick={() => onSelect(match)}
              >
                {#if options.show_line_numbers}
                  <span class="line">{match.line_number}</span>
                {/if}
                <span class="snippet">
                  {#each snippetParts(match, query) as part}
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

          {#if group.matches.length > MOBILE_MATCH_LIMIT && !expandedFiles.has(group.path)}
            <button class="show-more" type="button" onclick={() => toggleExpandedFile(group.path)}>
              Show {group.matches.length - MOBILE_MATCH_LIMIT} more
            </button>
          {/if}
        </section>
      {/each}
    </div>

    {#if currentFileRow && scrollTop > FILE_ROW_HEIGHT && options.group_by_file}
      <div class="current-file-header" aria-label="Current file">
        <div class="file-title">
          <strong title={currentFileRow.path}>{filename(currentFileRow.path)}</strong>
          <span title={parentPath(currentFileRow.path)}>{parentPath(currentFileRow.path)}</span>
        </div>
        <span class="file-actions">
          <span class="count">{matchLabel(currentFileRow.count)}</span>
          <button type="button" title="Open file" onclick={(event) => handleAction(event, () => onOpen(currentFileRow.path))}>Open</button>
          <details class="more-actions" ontoggle={handleMoreActionsToggle} onfocusout={handleMoreActionsFocusOut}>
            <summary title="More actions" aria-label="More actions">...</summary>
            <div class="menu">
              <button type="button" onclick={(event) => handleAction(event, () => onReveal(currentFileRow.path))}>Reveal</button>
              <button type="button" onclick={(event) => handleCopy(event, currentFileRow.path)}>Copy path</button>
              <button type="button" onclick={(event) => copyFilename(event, currentFileRow.path)}>Copy filename</button>
            </div>
          </details>
        </span>
      </div>
    {/if}

    <div class="virtual-list" style:height={`${totalHeight}px`}>
      {#each visibleRows as row (row.key)}
        {#if row.type === 'file'}
          <div class="file-row" style:transform={`translateY(${row.top}px)`}>
            <div class="file-title">
              <strong title={row.path}>{filename(row.path)}</strong>
              <span title={parentPath(row.path)}>{parentPath(row.path)}</span>
            </div>
            <span class="file-actions">
              <span class="count">{matchLabel(row.count)}</span>
              <button type="button" title="Open file" onclick={(event) => handleAction(event, () => onOpen(row.path))}>Open</button>
              <details class="more-actions" ontoggle={handleMoreActionsToggle} onfocusout={handleMoreActionsFocusOut}>
                <summary title="More actions" aria-label="More actions">...</summary>
                <div class="menu">
                  <button type="button" onclick={(event) => handleAction(event, () => onReveal(row.path))}>Reveal</button>
                  <button type="button" onclick={(event) => handleCopy(event, row.path)}>Copy path</button>
                  <button type="button" onclick={(event) => copyFilename(event, row.path)}>Copy filename</button>
                </div>
              </details>
            </span>
          </div>
        {:else}
          <div class="match-shell" style:transform={`translateY(${row.top}px)`}>
              <button
                type="button"
                class:selected={sameMatch(selected, row.match)}
                class:no-lines={!options.show_line_numbers}
                data-selected-match={sameMatch(selected, row.match) ? 'true' : undefined}
                class="match-row"
                onclick={() => onSelect(row.match)}
              >
                {#if options.show_line_numbers}
                  <span class="line">{row.match.line_number}</span>
                {/if}
                <span class="snippet">
                  {#each snippetParts(row.match, query) as part}
                    {#if part.hit}
                      <mark>{part.text}</mark>
                    {:else}
                      <span>{part.text}</span>
                    {/if}
                  {/each}
                </span>
              </button>
          </div>
        {/if}
      {/each}
    </div>

    {#if scrollScrubbing && currentLandmark}
      <div class="scroll-landmark" style={`top: ${scrubPreviewTop}px; left: ${scrubPreviewLeft}px;`} aria-hidden="true">
        <strong>{currentLandmark.cue}</strong>
        <span>{currentLandmark.primary}</span>
        <small>{currentLandmark.secondary}</small>
      </div>
    {/if}
  {/if}
</section>

<style>
  .results-panel {
    container-type: inline-size;
    --results-title-height: 68px;
    position: relative;
    isolation: isolate;
    min-width: 0;
    background: var(--surface);
    overflow: auto;
  }

  .panel-title {
    position: sticky;
    top: 0;
    z-index: 40;
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 6px;
    align-items: center;
    min-height: var(--results-title-height);
    border-bottom: 1px solid var(--border);
    padding: 6px 14px;
    background: var(--surface);
  }

  h2 {
    margin: 0;
    font-size: 14px;
    font-weight: 750;
  }

  .title-block {
    display: flex;
    gap: 8px;
    align-items: baseline;
    min-width: 0;
  }

  .title-block span {
    color: var(--muted);
    font-size: 12px;
    font-weight: 700;
  }

  .result-controls {
    display: flex;
    flex-wrap: nowrap;
    gap: 6px;
    justify-content: flex-start;
    min-width: 0;
  }

  .result-controls label {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    color: var(--muted);
    font-size: 11px;
    font-weight: 800;
  }

  .result-controls select {
    height: 28px;
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 0 7px;
    color: var(--text);
    background: var(--input);
    font: inherit;
    font-size: 11px;
    font-weight: 750;
  }

  .toggle-control {
    height: 28px;
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 0 8px;
    background: var(--input);
  }

  .toggle-control input {
    width: auto;
    height: auto;
  }

  .result-options-menu {
    position: relative;
    display: none;
  }

  .result-options-menu summary {
    display: inline-grid;
    width: 30px;
    height: 28px;
    border: 1px solid var(--border);
    border-radius: 5px;
    place-items: center;
    color: var(--text);
    background: var(--input);
    cursor: pointer;
    font-size: 13px;
    font-weight: 900;
    line-height: 1;
    list-style: none;
  }

  .result-options-menu summary::-webkit-details-marker {
    display: none;
  }

  .result-options-menu .menu {
    position: absolute;
    top: 32px;
    right: 0;
    z-index: 60;
    display: grid;
    min-width: 164px;
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 4px;
    background: var(--panel);
    box-shadow: 0 10px 24px rgba(30, 37, 45, 0.16);
  }

  .result-options-menu .toggle-control {
    justify-content: flex-start;
    border: 0;
    background: transparent;
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

  .current-file-header {
    position: sticky;
    top: var(--results-title-height);
    z-index: 30;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 10px;
    align-items: center;
    min-height: 46px;
    border-bottom: 1px solid var(--border);
    padding: 6px 11px;
    background: var(--panel);
    box-shadow: 0 1px 0 rgba(30, 37, 45, 0.04);
  }

  .current-file-header:has(details[open]) {
    z-index: 20;
  }

  .virtual-list {
    position: relative;
    z-index: 0;
    margin: 0 10px 10px;
    min-height: 0;
  }

  .scroll-landmark {
    position: fixed;
    z-index: 80;
    display: grid;
    width: min(228px, calc(100vw - 24px));
    min-height: 112px;
    transform: translateY(-50%);
    gap: 4px;
    border: 1px solid rgba(197, 204, 213, 0.86);
    border-radius: 8px;
    padding: 12px 14px;
    color: var(--text);
    background: rgba(255, 255, 255, 0.92);
    box-shadow: 0 18px 42px rgba(30, 37, 45, 0.22);
    pointer-events: none;
    backdrop-filter: blur(10px);
  }

  .scroll-landmark strong,
  .scroll-landmark span,
  .scroll-landmark small {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .scroll-landmark strong {
    font-size: 38px;
    line-height: 0.96;
    font-weight: 900;
    letter-spacing: 0;
  }

  .scroll-landmark span {
    font-size: 13px;
    font-weight: 850;
  }

  .scroll-landmark small {
    color: var(--muted);
    font-size: 12px;
    font-weight: 700;
  }

  .mobile-groups {
    display: none;
    position: relative;
    z-index: 0;
  }

  .file-row,
  .match-shell {
    position: absolute;
    top: 0;
    right: 0;
    left: 0;
  }

  .file-row:has(details[open]) {
    z-index: 25;
  }

  .file-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 10px;
    align-items: center;
    height: 44px;
    border-bottom: 1px solid var(--border-subtle);
    border-radius: 5px 5px 0 0;
    padding: 6px 11px;
    background: var(--panel);
    user-select: none;
  }

  .file-title {
    display: grid;
    gap: 2px;
    min-width: 0;
  }

  .file-title strong,
  .file-title span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .file-title strong {
    color: var(--text);
    font-size: 13px;
    font-weight: 850;
  }

  .file-title span {
    color: #7a8490;
    font-size: 11px;
    font-weight: 500;
  }

  .count {
    color: #7f8a94;
    font-size: 12px;
    font-weight: 650;
    text-align: center;
  }

  .file-actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .file-actions button,
  .file-actions summary {
    height: 24px;
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 0 7px;
    color: var(--text);
    background: var(--input);
    font: inherit;
    font-size: 11px;
    font-weight: 800;
  }

  .file-actions details {
    position: relative;
    z-index: 2;
  }

  .file-actions details[open] {
    z-index: 40;
  }

  .file-actions summary {
    display: inline-grid;
    width: 28px;
    padding: 0;
    place-items: center;
    cursor: pointer;
    list-style: none;
  }

  .file-actions summary::-webkit-details-marker {
    display: none;
  }

  .file-actions .menu {
    position: absolute;
    top: 28px;
    right: 0;
    z-index: 50;
    display: grid;
    min-width: 136px;
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 4px;
    background: var(--panel);
    box-shadow: 0 10px 24px rgba(30, 37, 45, 0.16);
  }

  .file-actions .menu button {
    height: 30px;
    border: 0;
    border-radius: 4px;
    padding: 0 8px;
    background: transparent;
    text-align: left;
  }

  .file-actions button:hover,
  .file-actions button:focus-visible,
  .file-actions summary:hover,
  .file-actions summary:focus-visible {
    border-color: var(--border-strong);
    outline: none;
  }

  .file-actions .menu button:hover,
  .file-actions .menu button:focus-visible {
    background: var(--selection);
    outline: none;
  }

  .match-row {
    position: relative;
    display: grid;
    grid-template-columns: 54px minmax(0, 1fr);
    gap: 8px;
    width: 100%;
    height: 28px;
    border: 0;
    border-bottom: 1px solid var(--border-subtle);
    border-radius: 0;
    padding: 4px 9px;
    color: var(--text);
    background: transparent;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .match-row.no-lines {
    grid-template-columns: minmax(0, 1fr);
  }

  .match-row:last-child {
    border-bottom: 0;
  }

  .match-row:hover,
  .match-row.selected {
    background: var(--selection);
  }

  .match-row.selected {
    box-shadow: inset 3px 0 0 var(--accent-strong);
  }

  .match-row.selected::before {
    content: "";
    position: absolute;
    top: 5px;
    bottom: 5px;
    left: 0;
    width: 3px;
    border-radius: 0 3px 3px 0;
    background: var(--accent-strong);
  }

  .line {
    color: #88939d;
    font-variant-numeric: tabular-nums;
    text-align: right;
  }

  .snippet {
    min-width: 0;
    overflow: hidden;
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: 12px;
    line-height: 17px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  mark {
    border-radius: 4px;
    padding: 0 2px;
    color: #241800;
    background: var(--highlight);
  }

  @media (max-width: 1199px) {
    .file-actions button {
      opacity: 1;
    }
  }

  @media (max-width: 599px) {
    .results-panel {
      --results-title-height: 72px;
    }

    .panel-title {
      grid-template-columns: minmax(0, 1fr);
      gap: 5px;
    }

    .result-controls {
      justify-content: flex-start;
    }

    .title-block {
      display: grid;
      gap: 1px;
    }

    .result-controls label:not(.toggle-control) > span {
      display: none;
    }

    .toggle-control span {
      display: inline;
    }

    .current-file-header {
      display: none;
    }

    .virtual-list {
      display: none;
    }

    .mobile-groups {
      display: block;
      padding: 0 0 8px;
    }

    .mobile-file-group {
      background: var(--panel);
    }

    .mobile-file-header {
      position: sticky;
      top: var(--results-title-height);
      z-index: 30;
      display: grid;
      grid-template-columns: minmax(0, 1fr) auto;
      gap: 8px;
      align-items: center;
      min-height: 46px;
      border-top: 1px solid var(--border-subtle);
      border-bottom: 1px solid var(--border);
      padding: 5px 8px;
      background: var(--panel);
      box-shadow: 0 1px 0 rgba(30, 37, 45, 0.04);
    }

    .mobile-file-header:has(details[open]) {
      z-index: 30;
    }

    .mobile-file-title {
      display: grid;
      gap: 1px;
      min-width: 0;
    }

    .mobile-file-title strong,
    .mobile-file-title span {
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .mobile-file-title strong {
      font-size: 13px;
      font-weight: 850;
    }

    .mobile-file-title span,
    .menu-title {
      color: #7a8490;
      font-size: 11px;
      font-weight: 550;
    }

    .mobile-file-actions {
      display: flex;
      align-items: center;
      gap: 5px;
    }

    .mobile-file-actions > button,
    .show-more {
      height: 26px;
      border: 1px solid var(--border);
      border-radius: 5px;
      padding: 0 7px;
      color: var(--text);
      background: var(--input);
      font: inherit;
      font-size: 11px;
      font-weight: 800;
    }

    details {
      position: relative;
      z-index: 1;
    }

    details[open] {
      z-index: 40;
    }

    summary {
      display: inline-grid;
      width: 28px;
      height: 26px;
      border: 1px solid var(--border);
      border-radius: 5px;
      place-items: center;
      color: var(--text);
      background: var(--input);
      cursor: pointer;
      font-size: 13px;
      font-weight: 900;
      list-style: none;
    }

    summary::-webkit-details-marker {
      display: none;
    }

    .menu {
      position: absolute;
      top: 30px;
      right: 0;
      z-index: 50;
      display: grid;
      min-width: 150px;
      border: 1px solid var(--border);
      border-radius: 6px;
      padding: 4px;
      background: var(--panel);
      box-shadow: 0 10px 24px rgba(30, 37, 45, 0.16);
    }

    .menu-title {
      border-bottom: 1px solid var(--border-subtle);
      padding: 5px 8px 7px;
      white-space: nowrap;
    }

    .menu button {
      height: 30px;
      border: 0;
      border-radius: 4px;
      padding: 0 8px;
      color: var(--text);
      background: transparent;
      font: inherit;
      font-size: 12px;
      font-weight: 700;
      text-align: left;
    }

    .menu button:hover,
    .menu button:focus-visible {
      background: var(--selection);
      outline: none;
    }

    .mobile-matches {
      padding: 0;
    }

    .mobile-match-row {
      grid-template-columns: 46px minmax(0, 1fr);
      height: 32px;
      padding: 6px 8px;
    }

    .show-more {
      width: calc(100% - 16px);
      margin: 7px 8px 10px;
      color: var(--muted);
      background: var(--surface);
    }
  }

  @container (max-width: 520px) {
    .result-controls > .toggle-control {
      display: none;
    }

    .result-options-menu {
      display: block;
    }
  }

  @container (max-width: 420px) {
    .current-file-header,
    .file-row {
      gap: 6px;
      padding-right: 8px;
      padding-left: 8px;
    }

    .count {
      display: none;
    }

    .file-title span {
      display: none;
    }
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
