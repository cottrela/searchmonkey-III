<script lang="ts">
  import { onMount, tick } from 'svelte';
  import type { FileResultGroup, SearchMatch, SearchState } from '$lib/types';

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

  const FULL_LINE_LIMIT = 200;
  const SNIPPET_CONTEXT = 64;
  const FILE_ROW_HEIGHT = 50;
  const MATCH_ROW_HEIGHT = 34;
  const OVERSCAN = 12;
  const MOBILE_MATCH_LIMIT = 10;

  let {
    groups,
    query,
    regex,
    selected,
    state: searchState,
    hasSearched,
    onSelect,
    onOpen,
    onReveal
  }: {
    groups: FileResultGroup[];
    query: string;
    regex: boolean;
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
  let expandedFiles = $state<Set<string>>(new Set());

  const rows = $derived.by(() => buildRows(groups));
  const totalHeight = $derived.by(() => {
    const last = rows.at(-1);
    return last ? last.top + last.height : 0;
  });
  const visibleRows = $derived.by(() => {
    const start = Math.max(0, scrollTop - OVERSCAN * MATCH_ROW_HEIGHT);
    const end = scrollTop + viewportHeight + OVERSCAN * MATCH_ROW_HEIGHT;

    return rows.filter((row) => row.top + row.height >= start && row.top <= end);
  });

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
      nextRows.push({
        type: 'file',
        key: `file:${group.path}`,
        path: group.path,
        count: group.matches.length,
        top,
        height: FILE_ROW_HEIGHT
      });
      top += FILE_ROW_HEIGHT;

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

  function copyText(text: string) {
    if (!text) return;
    void navigator.clipboard?.writeText(text);
  }

  function filename(filePath: string) {
    const parts = filePath.split('/').filter(Boolean);
    return parts.at(-1) || filePath;
  }

  function parentPath(filePath: string) {
    const slashIndex = filePath.lastIndexOf('/');
    if (slashIndex <= 0) return filePath;
    return filePath.slice(0, slashIndex);
  }

  function copyFilename(filePath: string) {
    copyText(filename(filePath));
  }

  function visibleMobileMatches(group: FileResultGroup) {
    if (expandedFiles.has(group.path)) return group.matches;
    return group.matches.slice(0, MOBILE_MATCH_LIMIT);
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

  function updateScrollMetrics() {
    if (!resultsElement) return;

    scrollTop = resultsElement.scrollTop;
    viewportHeight = resultsElement.clientHeight;
  }

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

<section bind:this={resultsElement} class="results-panel" aria-label="Search results" onscroll={updateScrollMetrics}>
  <div class="panel-title">
    <h2>Results</h2>
    {#if groups.length}
      <span>{groups.length} files</span>
    {/if}
  </div>

  {#if !hasSearched}
    <div class="empty">Choose a folder and search text files</div>
  {:else if searchState === 'searching' && groups.length === 0}
    <div class="empty active-search">
      <span class="spinner" aria-hidden="true"></span>
      <span>Searching current files...</span>
    </div>
  {:else if searchState === 'stopping' && groups.length === 0}
    <div class="empty active-search">
      <span class="spinner" aria-hidden="true"></span>
      <span>Stopping search...</span>
    </div>
  {:else if groups.length === 0}
    <div class="empty">No matches found</div>
  {:else}
    <div class="mobile-groups">
      {#each groups as group (group.path)}
        <section class="mobile-file-group" aria-label={filename(group.path)}>
          <div class="mobile-file-header">
            <div class="mobile-file-title">
              <strong title={group.path}>{filename(group.path)}</strong>
              <span title={parentPath(group.path)}>{parentPath(group.path)}</span>
            </div>
            <div class="mobile-file-actions">
              <button type="button" onclick={() => onOpen(group.path)}>Open</button>
              <details>
                <summary title="More actions" aria-label="More actions">...</summary>
                <div class="menu">
                  <div class="menu-title">{matchLabel(group.matches.length)}</div>
                  <button type="button" onclick={() => onOpen(group.path)}>Open</button>
                  <button type="button" onclick={() => onReveal(group.path)}>Reveal</button>
                  <button type="button" onclick={() => copyText(group.path)}>Copy path</button>
                  <button type="button" onclick={() => copyText(filename(group.path))}>Copy filename</button>
                  <button type="button" onclick={() => toggleExpandedFile(group.path)}>
                    {expandedFiles.has(group.path) ? 'Collapse file' : 'Show all matches'}
                  </button>
                </div>
              </details>
            </div>
          </div>

          <div class="mobile-matches">
            {#each visibleMobileMatches(group) as match, index (`${match.path}:${match.line_number}:${index}`)}
              <button
                type="button"
                class:selected={sameMatch(selected, match)}
                data-selected-match={sameMatch(selected, match) ? 'true' : undefined}
                class="match-row mobile-match-row"
                onclick={() => onSelect(match)}
              >
                <span class="line">{match.line_number}</span>
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
              <button type="button" title="Open file" onclick={() => onOpen(row.path)}>Open</button>
              <button class="hover-action" type="button" title="Reveal file" onclick={() => onReveal(row.path)}>Reveal</button>
              <button class="hover-action" type="button" title="Copy path" onclick={() => copyText(row.path)}>Copy</button>
            </span>
          </div>
        {:else}
          <div class="match-shell" style:transform={`translateY(${row.top}px)`}>
              <button
                type="button"
                class:selected={sameMatch(selected, row.match)}
                data-selected-match={sameMatch(selected, row.match) ? 'true' : undefined}
                class="match-row"
                onclick={() => onSelect(row.match)}
              >
                <span class="line">{row.match.line_number}</span>
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
  {/if}
</section>

<style>
  .results-panel {
    min-width: 0;
    background: var(--surface);
    overflow: auto;
  }

  .panel-title {
    position: sticky;
    top: 0;
    z-index: 1;
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 47px;
    border-bottom: 1px solid var(--border);
    padding: 0 14px;
    background: var(--surface);
  }

  h2 {
    margin: 0;
    font-size: 14px;
  }

  .panel-title span {
    color: var(--muted);
    font-size: 12px;
    font-weight: 700;
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

  .virtual-list {
    position: relative;
    margin: 10px;
    min-height: 0;
  }

  .mobile-groups {
    display: none;
  }

  .file-row,
  .match-shell {
    position: absolute;
    top: 0;
    right: 0;
    left: 0;
  }

  .file-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 10px;
    align-items: center;
    height: 50px;
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
    font-weight: 800;
  }

  .file-title span {
    color: var(--muted);
    font-size: 11px;
    font-weight: 650;
  }

  .count {
    color: var(--muted);
    font-size: 12px;
    font-weight: 800;
    text-align: center;
  }

  .file-actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .file-actions button {
    height: 24px;
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 0 7px;
    color: var(--text);
    background: var(--input);
    font: inherit;
    font-size: 11px;
    font-weight: 800;
    transition: opacity 0.12s ease;
  }

  .file-actions .hover-action {
    opacity: 0;
  }

  .file-row:hover .hover-action,
  .file-row:focus-within .hover-action {
    opacity: 1;
  }

  .file-actions button:hover,
  .file-actions button:focus-visible {
    border-color: var(--border-strong);
    outline: none;
  }

  .match-row {
    display: grid;
    grid-template-columns: 54px minmax(0, 1fr);
    gap: 8px;
    width: 100%;
    height: 34px;
    border: 0;
    border-bottom: 1px solid var(--border-subtle);
    border-radius: 0;
    padding: 7px 9px;
    color: var(--text);
    background: transparent;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .match-row:last-child {
    border-bottom: 0;
  }

  .match-row:hover,
  .match-row.selected {
    background: var(--selection);
  }

  .line {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }

  .snippet {
    min-width: 0;
    overflow: hidden;
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: 12px;
    line-height: 19px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  mark {
    border-radius: 3px;
    padding: 0 1px;
    color: #241800;
    background: #ffd86b;
  }

  @media (max-width: 1199px) {
    .file-actions button {
      opacity: 1;
    }
  }

  @media (max-width: 599px) {
    .panel-title {
      height: 38px;
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
      top: 38px;
      z-index: 2;
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
    }

    .mobile-file-title span,
    .menu-title {
      color: var(--muted);
      font-size: 11px;
      font-weight: 700;
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
      height: 36px;
      padding: 8px;
    }

    .show-more {
      width: calc(100% - 16px);
      margin: 7px 8px 10px;
      color: var(--muted);
      background: var(--surface);
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
