<script lang="ts">
  import { tick } from 'svelte';
  import type { PreviewState } from '$lib/types';
  import { copyText } from '$lib/clipboard';
  import { filename, parentPath } from '$lib/paths';

  type Segment = {
    kind: 'text';
    text: string;
    match: boolean;
    active: boolean;
  };

  type SourceLine = {
    number: number;
    text: string;
    isMatch: boolean;
    isActive: boolean;
    matchRanges: Array<{ start: number; end: number }>;
    pageBreaks: Array<{ page?: number | null; label?: string | null }>;
  };

  type RenderLine =
    | (SourceLine & {
        key: string;
        kind: 'text';
        segments: Segment[];
      })
    | {
        key: string;
        kind: 'page-break';
        pageNumber: number;
        label: string;
      };

  const WRAP_LIMIT = 50_000;

  let {
    preview,
    errorMessage,
    activeFileMatchNumber,
    activeFileMatchTotal,
    canNavigateFiles,
    reindexingPaths,
    drilldown = false,
    onPrevious,
    onNext,
    onPreviousFile,
    onNextFile,
    onSelect,
    onOpen,
    onReveal,
    onReindex,
    onClose
  }: {
    preview: PreviewState;
    errorMessage: string;
    activeFileMatchNumber: number;
    activeFileMatchTotal: number;
    canNavigateFiles: boolean;
    reindexingPaths: Set<string>;
    drilldown?: boolean;
    onPrevious: () => void;
    onNext: () => void;
    onPreviousFile: () => void;
    onNextFile: () => void;
    onSelect: (match: PreviewState['matches'][number]) => void;
    onOpen: (path: string) => void;
    onReveal: (path: string) => void;
    onReindex: (match: PreviewState['matches'][number]) => void;
    onClose: () => void;
  } = $props();

  let previewElement = $state<HTMLDivElement>();
  let previewPanelElement = $state<HTMLElement>();
  let lastScrolledTarget = '';
  let wrapLines = $state(false);

  const IMAGE_EXTENSIONS = new Set([
    'png',
    'jpg',
    'jpeg',
    'gif',
    'webp',
    'bmp',
    'tif',
    'tiff',
    'avif',
    'heic',
    'heif'
  ]);

  const canNavigateMatches = $derived(activeFileMatchTotal > 1);

  const sourceLines = $derived.by(() =>
    buildSourceLines(preview.filePreview, preview.matches, preview.activeMatch)
  );
  const previewTextLength = $derived.by(() =>
    sourceLines.reduce((length, line) => length + line.text.length + 1, 0)
  );
  const canWrap = $derived(previewTextLength < WRAP_LIMIT);
  const effectiveWrap = $derived(wrapLines && canWrap);
  const renderLines = $derived.by(() => buildRenderLines(sourceLines));
  const activeMatchText = $derived(preview.activeMatch?.line_text ?? '');
  const activeMatchReindexing = $derived(Boolean(preview.activeMatch && reindexingPaths.has(preview.activeMatch.path)));
  const previewIsImage = $derived.by(() => isImagePath(preview.thumbnailPath));
  const activeMatchOnly = $derived.by(() => {
    const match = preview.activeMatch;
    if (!match?.submatches.length) return activeMatchText;
    return match.submatches.map((range) => match.line_text.slice(range.start, range.end)).join(' ');
  });

  function isImagePath(filePath: string) {
    const extension = filePath.split('.').at(-1)?.toLowerCase() ?? '';
    return IMAGE_EXTENSIONS.has(extension);
  }

  function buildSourceLines(
    filePreview: PreviewState['filePreview'],
    matches: PreviewState['matches'],
    active: PreviewState['matches'][number] | null
  ): SourceLine[] {
    const matchesByLine = new Map<number, PreviewState['matches']>();

    for (const match of matches) {
      const lineMatches = matchesByLine.get(match.line_number);

      if (lineMatches) {
        lineMatches.push(match);
      } else {
        matchesByLine.set(match.line_number, [match]);
      }
    }

    return (
      filePreview?.lines.map((line) => ({
        number: line.number,
        text: line.text,
        isMatch: matchesByLine.has(line.number),
        isActive: active?.line_number === line.number,
        matchRanges: mergeMatchRanges(matchesByLine.get(line.number) ?? []),
        pageBreaks: line.page_breaks ?? []
      })) ?? []
    );
  }

  function mergeMatchRanges(matches: PreviewState['matches']): Array<{ start: number; end: number }> {
    const ranges = matches
      .flatMap((match) => match.submatches)
      .sort((a, b) => a.start - b.start || a.end - b.end);
    const merged: Array<{ start: number; end: number }> = [];

    for (const range of ranges) {
      const previous = merged.at(-1);

      if (previous && range.start <= previous.end) {
        previous.end = Math.max(previous.end, range.end);
      } else {
        merged.push({ start: range.start, end: range.end });
      }
    }

    return merged;
  }

  function buildRenderLines(lines: SourceLine[]): RenderLine[] {
    const rendered: RenderLine[] = [];

    for (const line of lines) {
      const { lines: splitLines } = splitLine(line);
      rendered.push(...splitLines);
    }

    return rendered;
  }

  function splitLine(line: SourceLine): { lines: RenderLine[] } {
    const pieces = line.text.split('\f');
    const rendered: RenderLine[] = [];
    let pieceStart = 0;

    for (let index = 0; index < pieces.length; index += 1) {
      const piece = pieces[index];
      const pieceEnd = pieceStart + piece.length;
      rendered.push({
        ...line,
        key: `${line.number}:${index}`,
        kind: 'text',
        text: piece,
        segments: splitTextSegment(
          piece,
          line.matchRanges
            .map((range) => ({
              start: Math.max(range.start, pieceStart) - pieceStart,
              end: Math.min(range.end, pieceEnd) - pieceStart
            }))
            .filter((range) => range.start < range.end),
          line.isActive
        )
      });

      if (index < pieces.length - 1) {
        const pageBreak = line.pageBreaks[index];
        rendered.push({
          key: `${line.number}:page:${index}`,
          kind: 'page-break',
          pageNumber: pageBreak?.page ?? 0,
          label: pageBreak?.label ?? (pageBreak?.page ? `Page ${pageBreak.page}` : 'Page break')
        });
      }

      pieceStart = pieceEnd + 1;
    }

    return { lines: rendered };
  }

  function splitTextSegment(
    text: string,
    matchRanges: Array<{ start: number; end: number }>,
    active: boolean
  ): Segment[] {
    if (!matchRanges.length) return [{ kind: 'text', text, match: false, active }];

    const segments: Segment[] = [];
    let cursor = 0;

    for (const range of matchRanges) {
      const start = Math.max(0, Math.min(range.start, text.length));
      const end = Math.max(start, Math.min(range.end, text.length));

      if (end <= cursor) {
        continue;
      }

      if (start > cursor) {
        segments.push({ kind: 'text', text: text.slice(cursor, start), match: false, active });
      }

      segments.push({
        kind: 'text',
        text: text.slice(Math.max(start, cursor), end),
        match: true,
        active
      });
      cursor = end;
    }

    if (cursor < text.length) {
      segments.push({ kind: 'text', text: text.slice(cursor), match: false, active });
    }

    return segments.length ? segments : [{ kind: 'text', text, match: false, active }];
  }

  function selectLineMatch(lineNumber: number) {
    if (!selectionIsCollapsed()) return;

    const lineMatches = preview.matches.filter((match) => match.line_number === lineNumber);
    if (!lineMatches.length) return;

    const activeIndex = lineMatches.findIndex((match) => match === preview.activeMatch);
    onSelect(lineMatches[activeIndex >= 0 ? activeIndex : 0]);
  }

  function handleLineKeydown(event: KeyboardEvent, lineNumber: number) {
    if (event.key !== 'Enter' && event.key !== ' ') return;

    event.preventDefault();
    selectLineMatch(lineNumber);
  }

  function selectionIsCollapsed() {
    const selection = window.getSelection();
    return !selection || selection.isCollapsed;
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

  function closeMoreActionMenus(except?: HTMLDetailsElement) {
    previewPanelElement?.querySelectorAll<HTMLDetailsElement>('.more-actions[open]').forEach((menu) => {
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
    const handlePointerDown = (event: PointerEvent) => {
      if (!previewPanelElement) return;
      if (!(event.target instanceof Node)) return;

      const actionMenu = (event.target instanceof Element ? event.target : event.target.parentElement)?.closest('.more-actions');
      if (actionMenu && previewPanelElement.contains(actionMenu)) return;

      closeMoreActionMenus();
    };

    document.addEventListener('pointerdown', handlePointerDown, true);
    return () => {
      document.removeEventListener('pointerdown', handlePointerDown, true);
    };
  });

  $effect(() => {
    const target = preview.filePreview
      ? `${preview.filePath}:${preview.filePreview.start_line}:${preview.filePreview.end_line}:${preview.activeMatch?.line_number ?? 0}:${previewTextLength}`
      : '';
    if (!previewElement || !sourceLines.length || target === lastScrolledTarget) return;

    lastScrolledTarget = target;

    tick().then(() => {
      const activeLine = previewElement?.querySelector<HTMLElement>("[data-active-match='true']");
      if (!activeLine) return;

      activeLine.scrollIntoView({ block: 'center' });
    });
  });

</script>

<aside bind:this={previewPanelElement} class="preview-panel" class:drilldown aria-label="Match preview">
  {#if preview.filePath}
    <div class="mobile-preview-toolbar">
      <button type="button" onclick={onClose} title="Back to results">← Results</button>
      <span></span>
      <div class="drilldown-actions">
        <button type="button" onclick={(event) => handleAction(event, () => onOpen(preview.filePath))} title="Open file">Open</button>
        <button type="button" onclick={(event) => handleAction(event, () => onReveal(preview.filePath))}>Reveal</button>
        <button type="button" onclick={(event) => handleCopy(event, activeMatchOnly)} disabled={!activeMatchOnly}>
          Copy match
        </button>
        <button type="button" onclick={(event) => handleCopy(event, activeMatchText)} disabled={!activeMatchText}>
          Copy line
        </button>
        <button type="button" onclick={(event) => handleCopy(event, preview.filePath)}>Copy path</button>
        <button type="button" onclick={() => (wrapLines = !wrapLines)} disabled={!canWrap}>
          {effectiveWrap ? 'Disable wrap' : 'Wrap'}
        </button>
      </div>
      <details class="compact-actions more-actions" ontoggle={handleMoreActionsToggle} onfocusout={handleMoreActionsFocusOut}>
        <summary title="More actions" aria-label="More actions">...</summary>
        <div class="menu">
          <button type="button" onclick={(event) => handleAction(event, () => onOpen(preview.filePath))} title="Open file">Open</button>
          <button type="button" onclick={(event) => handleAction(event, () => onReveal(preview.filePath))}>Reveal</button>
          <button class="file-menu-action" type="button" onclick={onPreviousFile} disabled={!canNavigateFiles}>Previous file</button>
          <button class="file-menu-action" type="button" onclick={onNextFile} disabled={!canNavigateFiles}>Next file</button>
          <button type="button" onclick={(event) => handleCopy(event, activeMatchOnly)} disabled={!activeMatchOnly}>
            Copy match
          </button>
          <button type="button" onclick={(event) => handleCopy(event, activeMatchText)} disabled={!activeMatchText}>
            Copy line
          </button>
          <button type="button" onclick={(event) => handleCopy(event, preview.filePath)}>Copy path</button>
          <button type="button" onclick={() => (wrapLines = !wrapLines)} disabled={!canWrap}>
            {effectiveWrap ? 'Disable wrap' : 'Toggle wrap'}
          </button>
        </div>
      </details>
      <details class="mobile-actions more-actions" ontoggle={handleMoreActionsToggle} onfocusout={handleMoreActionsFocusOut}>
        <summary title="More actions" aria-label="More actions">...</summary>
        <div class="menu">
          <button type="button" onclick={(event) => handleAction(event, () => onOpen(preview.filePath))} title="Open file">Open</button>
          <button type="button" onclick={(event) => handleAction(event, () => onReveal(preview.filePath))}>Reveal</button>
          <button class="file-menu-action" type="button" onclick={onPreviousFile} disabled={!canNavigateFiles}>Previous file</button>
          <button class="file-menu-action" type="button" onclick={onNextFile} disabled={!canNavigateFiles}>Next file</button>
          <button type="button" onclick={(event) => handleCopy(event, activeMatchOnly)} disabled={!activeMatchOnly}>
            Copy match
          </button>
          <button type="button" onclick={(event) => handleCopy(event, activeMatchText)} disabled={!activeMatchText}>
            Copy line
          </button>
          <button type="button" onclick={(event) => handleCopy(event, preview.filePath)}>Copy path</button>
          <button type="button" onclick={() => (wrapLines = !wrapLines)} disabled={!canWrap}>
            {effectiveWrap ? 'Disable wrap' : 'Toggle wrap'}
          </button>
        </div>
      </details>
    </div>
    <div class="mobile-preview-file">
      <strong title={preview.filePath}>{filename(preview.filePath)}</strong>
      <span title={parentPath(preview.filePath)}>{parentPath(preview.filePath)}</span>
    </div>
  {/if}

  <div class="panel-title">
    {#if preview.filePath}
      <div class="desktop-preview-file">
        <div class="desktop-preview-title">
          <h2 title={preview.filePath}>{filename(preview.filePath)}</h2>
          {#if preview.activeMatch && (preview.activeMatch.meta_outdated || activeMatchReindexing)}
            <button
              type="button"
              class="meta-hint"
              onclick={() => preview.activeMatch && onReindex(preview.activeMatch)}
              disabled={activeMatchReindexing}
              title={activeMatchReindexing ? 'Re-index request queued' : 'Re-index this file'}
            >
              {activeMatchReindexing ? 'Queued for re-index' : 'Re-index file?'}
            </button>
          {/if}
        </div>
        <div class="desktop-preview-path" title={parentPath(preview.filePath)}>
          {parentPath(preview.filePath)}
        </div>
      </div>
      <div class="desktop-header-nav">
        <div class="match-nav" aria-label="Match navigation">
          <button class="file-nav-button" type="button" onclick={onPreviousFile} disabled={!canNavigateFiles} title="Previous file"><span class="nav-label-full">‹ File</span><span class="nav-label-short">‹</span></button>
          <button class="match-nav-button" type="button" onclick={onPrevious} disabled={!canNavigateMatches} title="Previous match (Shift+Enter / Shift+F4)"><span class="nav-label-full">‹ Match</span><span class="nav-label-short">‹</span></button>
          <span>{activeFileMatchNumber} / {activeFileMatchTotal}</span>
          <button class="match-nav-button" type="button" onclick={onNext} disabled={!canNavigateMatches} title="Next match (Enter / F4)"><span class="nav-label-full">Match ›</span><span class="nav-label-short">›</span></button>
          <button class="file-nav-button" type="button" onclick={onNextFile} disabled={!canNavigateFiles} title="Next file"><span class="nav-label-full">File ›</span><span class="nav-label-short">›</span></button>
        </div>
      </div>
      <div class="desktop-preview-actions">
        <button type="button" onclick={(event) => handleAction(event, () => onOpen(preview.filePath))} title="Open file">Open</button>
        <button class="reveal-action" type="button" onclick={(event) => handleAction(event, () => onReveal(preview.filePath))} title="Reveal file">Reveal</button>
        <details class="more-actions" ontoggle={handleMoreActionsToggle} onfocusout={handleMoreActionsFocusOut}>
          <summary title="More actions" aria-label="More actions">...</summary>
          <div class="menu">
            <button class="reveal-menu-action" type="button" onclick={(event) => handleAction(event, () => onReveal(preview.filePath))}>Reveal</button>
            <button class="file-menu-action" type="button" onclick={onPreviousFile} disabled={!canNavigateFiles}>Previous file</button>
            <button class="file-menu-action" type="button" onclick={onNextFile} disabled={!canNavigateFiles}>Next file</button>
            <button type="button" onclick={(event) => handleCopy(event, activeMatchOnly)} disabled={!activeMatchOnly}>
              Copy match
            </button>
            <button type="button" onclick={(event) => handleCopy(event, preview.filePath)}>Copy path</button>
            <button type="button" onclick={() => (wrapLines = !wrapLines)} disabled={!canWrap}>
              {effectiveWrap ? 'Disable wrap' : 'Toggle wrap'}
            </button>
          </div>
        </details>
      </div>
    {:else}
      <h2>Preview</h2>
    {/if}
  </div>

  {#if errorMessage}
    <div class="empty">{errorMessage}</div>
  {:else if preview.filePath}
    <div class="preview-body">
      {#if !canWrap}
        <div class="wrap-message">Line wrapping is disabled for previews over 50,000 characters.</div>
      {/if}
      <div class="preview-scroll">
        {#if preview.filePreview}
          <div
            bind:this={previewElement}
            class="preview"
            class:wrap={effectiveWrap}
          >
            {#each renderLines as line (line.key)}
              {#if line.kind === 'page-break'}
                <div class="page-break-line" aria-hidden="true">
                  <span class="page-break-gutter"></span>
                  <code class="source"><span class="page-break">──────── {line.label} ────────</span></code>
                </div>
              {:else if line.isMatch}
                <div
                  class="line"
                  role="button"
                  tabindex="0"
                  data-match="true"
                  data-active-match={line.isActive ? 'true' : undefined}
                  onclick={() => selectLineMatch(line.number)}
                  onkeydown={(event) => handleLineKeydown(event, line.number)}
                >
                  <span class="gutter" aria-hidden="true" data-line-number={line.number}></span>
                  <code class="source">{#each line.segments as segment}{#if segment.match}<span class:active={segment.active} class="match">{segment.text}</span>{:else}<span>{segment.text}</span>{/if}{/each}</code>
                </div>
              {:else}
                <div class="line">
                  <span class="gutter" aria-hidden="true" data-line-number={line.number}></span>
                  <code class="source">{#each line.segments as segment}{#if segment.match}<span class:active={segment.active} class="match">{segment.text}</span>{:else}<span>{segment.text}</span>{/if}{/each}</code>
                </div>
              {/if}
            {/each}
          </div>
        {:else}
          <div class="empty inline">Reading file...</div>
        {/if}
      </div>

      <div class="mobile-match-nav">
        <button class="file-nav-button" type="button" onclick={onPreviousFile} disabled={!canNavigateFiles} title="Previous file"><span class="nav-label-full">‹ File</span><span class="nav-label-short">‹</span></button>
        <button class="match-nav-button" type="button" onclick={onPrevious} disabled={!canNavigateMatches} title="Previous match (Shift+Enter / Shift+F4)"><span class="nav-label-full">‹ Match</span><span class="nav-label-short">‹</span></button>
        <span>{activeFileMatchNumber} / {activeFileMatchTotal}</span>
        <button class="match-nav-button" type="button" onclick={onNext} disabled={!canNavigateMatches} title="Next match (Enter / F4)"><span class="nav-label-full">Match ›</span><span class="nav-label-short">›</span></button>
        <button class="file-nav-button" type="button" onclick={onNextFile} disabled={!canNavigateFiles} title="Next file"><span class="nav-label-full">File ›</span><span class="nav-label-short">›</span></button>
      </div>

    </div>
  {:else}
    <div class="empty">Select a match to preview it</div>
  {/if}
</aside>

<style>
  .preview-panel {
    container-type: inline-size;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    min-width: 0;
    min-height: 0;
    background: var(--preview-bg);
    box-shadow: inset 1px 0 0 var(--border);
    overflow: hidden;
  }

  .panel-title {
    display: grid;
    grid-template-columns: minmax(120px, 1fr) auto auto;
    gap: 8px;
    align-items: center;
    min-height: 56px;
    border-bottom: 1px solid var(--border);
    padding: 7px 10px 7px 14px;
    background: var(--preview-bg);
  }

  .desktop-preview-file {
    display: grid;
    gap: 2px;
    min-width: 0;
    max-width: 100%;
  }

  .desktop-preview-title {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 10px;
  }

  .meta-hint {
    display: inline-flex;
    align-items: center;
    flex: 0 0 auto;
    border: 0;
    padding: 0;
    color: var(--muted);
    background: transparent;
    font: inherit;
    font-size: 11px;
    font-weight: 700;
    text-decoration: underline;
    text-decoration-style: dotted;
    text-underline-offset: 0.18em;
    white-space: nowrap;
    cursor: pointer;
  }

  .meta-hint:hover,
  .meta-hint:focus-visible {
    color: var(--text);
    outline: none;
  }

  h2 {
    min-width: 0;
    margin: 0;
    font-size: 14px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .desktop-preview-path {
    min-width: 0;
    overflow: hidden;
    color: var(--muted);
    font-size: 11px;
    font-weight: 650;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .desktop-preview-actions {
    display: flex;
    flex-wrap: nowrap;
    justify-content: flex-end;
    gap: 4px;
    min-width: max-content;
    white-space: nowrap;
  }

  .desktop-header-nav {
    display: flex;
    justify-content: center;
    min-width: max-content;
    white-space: nowrap;
  }

  .match-nav {
    display: grid;
    grid-template-columns: auto auto minmax(76px, max-content) auto auto;
    align-items: center;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--input);
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.55);
    overflow: hidden;
  }

  .match-nav button {
    height: 26px;
    border: 0;
    border-radius: 0;
    padding: 0 9px;
    color: var(--text);
    background: transparent;
    font-size: 11px;
    font-weight: 800;
  }

  .match-nav > * + * {
    border-left: 1px solid rgba(217, 222, 229, 0.72);
  }

  .nav-label-short {
    display: none;
  }

  .match-nav .file-nav-button {
    color: #78838e;
    font-weight: 650;
  }

  .match-nav button:disabled {
    color: #a4adb6;
  }

  .match-nav button:not(:disabled):hover,
  .match-nav button:not(:disabled):focus-visible {
    background: var(--selection);
    outline: none;
  }

  .match-nav button:not(:disabled):active {
    background: var(--selection-strong);
  }

  .match-nav span {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    font-size: 11px;
    font-weight: 800;
    padding: 0 8px;
    text-align: center;
    white-space: nowrap;
  }

  .file-menu-action {
    display: none;
  }

  .reveal-menu-action {
    display: none;
  }

  .desktop-preview-actions details {
    position: relative;
  }

  .desktop-preview-actions summary {
    display: inline-grid;
    width: 30px;
    height: 30px;
    border: 1px solid var(--border);
    border-radius: 6px;
    place-items: center;
    color: var(--text);
    background: var(--input);
    cursor: pointer;
    font-size: 13px;
    font-weight: 900;
    list-style: none;
  }

  .desktop-preview-actions summary::-webkit-details-marker {
    display: none;
  }

  .desktop-preview-actions .menu {
    position: absolute;
    top: 34px;
    right: 0;
    z-index: 6;
    display: grid;
    min-width: 142px;
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 4px;
    background: var(--preview-bg);
    box-shadow: 0 10px 24px rgba(30, 37, 45, 0.16);
  }

  .desktop-preview-actions .menu button {
    height: 30px;
    border: 0;
    border-radius: 4px;
    padding: 0 8px;
    background: transparent;
    text-align: left;
  }

  .desktop-preview-actions .menu button:hover,
  .desktop-preview-actions .menu button:focus-visible {
    background: var(--selection);
    outline: none;
  }

  .mobile-preview-toolbar,
  .mobile-preview-file,
  .mobile-match-nav {
    display: none;
  }

  .preview-panel.drilldown {
    grid-template-rows: auto auto minmax(0, 1fr);
  }

  .preview-panel.drilldown .panel-title {
    display: none;
  }

  .preview-panel.drilldown .mobile-preview-toolbar {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    gap: 8px;
    align-items: center;
    min-height: 38px;
    border-bottom: 1px solid var(--border);
    padding: 5px 8px;
    background: var(--panel);
  }

  .drilldown-actions {
    display: none;
    flex-wrap: wrap;
    gap: 6px;
    justify-content: flex-end;
  }

  .compact-actions,
  .mobile-actions {
    display: none;
  }

  .preview-panel.drilldown .drilldown-actions {
    display: flex;
  }

  .preview-panel.drilldown .compact-actions {
    display: none;
  }

  .preview-panel.drilldown .mobile-preview-file {
    display: grid;
    gap: 1px;
    border-bottom: 1px solid var(--border-subtle);
    padding: 6px 8px;
    background: var(--surface);
  }

  .preview-panel.drilldown .mobile-preview-file strong,
  .preview-panel.drilldown .mobile-preview-file span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .preview-panel.drilldown .mobile-preview-file strong {
    font-size: 13px;
  }

  .preview-panel.drilldown .mobile-preview-file span {
    color: var(--muted);
    font-size: 11px;
    font-weight: 700;
  }

  .preview-body {
    min-height: 0;
    padding: 14px;
    overflow: hidden;
  }

  .preview-scroll {
    min-height: 0;
  }

  .wrap-message {
    margin-top: 6px;
    color: var(--muted);
    font-size: 12px;
    line-height: 16px;
  }

  .preview {
    flex: 1 1 auto;
    min-height: 0;
    margin: 12px 0 0;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: auto;
    color: var(--text);
    background: var(--code-bg);
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: 13px;
    line-height: 17px;
    contain: content;
  }

  .line {
    position: relative;
    display: grid;
    grid-template-columns: 44px minmax(0, 1fr);
    min-width: max-content;
    min-height: 17px;
  }

  .line[data-match='true'] {
    cursor: pointer;
  }

  .gutter {
    border-right: 1px solid var(--border-subtle);
    padding: 0 6px 0 4px;
    color: var(--muted);
    background: #edf2f0;
    text-align: right;
    font-variant-numeric: tabular-nums;
    pointer-events: none;
    user-select: none;
  }

  .gutter::before {
    content: attr(data-line-number);
  }

  .line[data-active-match='true'] {
    background: var(--highlight-row);
    outline: 1px solid #c78413;
    outline-offset: -1px;
  }

  .line[data-active-match='true']::before {
    content: "";
    position: absolute;
    top: 2px;
    bottom: 2px;
    left: 0;
    width: 3px;
    border-radius: 0 3px 3px 0;
    background: var(--accent-strong);
    z-index: 1;
  }

  .line[data-match='true']:not([data-active-match='true']) {
    background: var(--highlight-row-soft);
  }

  .page-break-line {
    display: grid;
    grid-template-columns: 44px minmax(0, 1fr);
    min-width: max-content;
    min-height: 17px;
  }

  .page-break-gutter {
    border-right: 1px solid var(--border-subtle);
    background: #edf2f0;
  }

  .source {
    padding: 0 8px;
    overflow-wrap: normal;
    white-space: pre;
    word-break: normal;
    user-select: text;
  }

  .page-break {
    color: var(--muted);
  }

  .preview.wrap .line {
    min-width: 0;
  }

  .preview.wrap .source {
    min-width: 0;
    white-space: pre-wrap;
  }

  .match {
    border-radius: 4px;
    padding: 0 2px;
    color: #241800;
    background: var(--highlight);
  }

  .match.active {
    outline: 1px solid #b06b00;
    background: var(--highlight-strong);
  }

  button {
    height: 30px;
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0 9px;
    color: var(--text);
    background: var(--input);
    font: inherit;
    font-size: 12px;
    font-weight: 700;
  }

  button:not(:disabled) {
    cursor: pointer;
  }

  button:disabled {
    color: var(--muted);
    background: var(--disabled);
  }

  .empty {
    display: grid;
    min-height: 220px;
    place-items: center;
    padding: 24px;
    color: var(--muted);
    text-align: center;
  }

  .empty.inline {
    min-height: 180px;
  }

  @media (max-width: 849px) {
    .panel-title {
      grid-template-columns: minmax(120px, 1fr) auto auto;
    }

    .desktop-preview-actions {
      justify-content: flex-end;
    }

  }

  @media (max-width: 599px) {
    .preview-panel {
      grid-template-rows: auto auto minmax(0, 1fr);
    }

    .panel-title {
      display: none;
    }

    .mobile-preview-toolbar {
      display: grid;
      grid-template-columns: auto minmax(0, 1fr) auto auto;
      gap: 8px;
      align-items: center;
      min-height: 38px;
      border-bottom: 1px solid var(--border);
      padding: 5px 8px;
      background: var(--panel);
    }

    .drilldown-actions,
    .compact-actions,
    .preview-panel.drilldown .drilldown-actions,
    .preview-panel.drilldown .compact-actions {
      display: none;
    }

    .mobile-actions {
      display: block;
    }

    .mobile-preview-toolbar > span {
      color: var(--muted);
      font-size: 12px;
      font-weight: 800;
      text-align: center;
    }

    .mobile-preview-file {
      display: grid;
      gap: 1px;
      border-bottom: 1px solid var(--border-subtle);
      padding: 6px 8px;
      background: var(--surface);
    }

    .mobile-preview-file strong,
    .mobile-preview-file span {
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .mobile-preview-file strong {
      font-size: 13px;
    }

    .mobile-preview-file span {
      color: var(--muted);
      font-size: 11px;
      font-weight: 700;
    }

    .panel-title {
      min-height: 42px;
      padding: 5px 8px;
    }

    .desktop-preview-actions {
      gap: 4px;
    }

    button {
      height: 28px;
      padding: 0 7px;
      font-size: 11px;
    }

    .preview-body {
      padding: 8px;
    }

    .wrap-message {
      display: none;
    }

    .preview {
      margin-top: 0;
      font-size: 12px;
      line-height: 17px;
    }

    .mobile-match-nav {
      display: grid;
      grid-template-columns: auto auto minmax(72px, 1fr) auto auto;
      gap: 0;
      align-items: center;
      margin-top: 8px;
      overflow: hidden;
      border: 1px solid var(--border);
      border-radius: 6px;
      background: var(--input);
    }

    .mobile-match-nav button {
      height: 32px;
      border: 0;
      border-radius: 0;
      padding: 0 7px;
      background: transparent;
      font-size: 11px;
      font-weight: 800;
    }

    .mobile-match-nav span {
      color: var(--muted);
      font-variant-numeric: tabular-nums;
      font-size: 12px;
      font-weight: 800;
      padding: 0 5px;
      text-align: center;
    }

    .mobile-match-nav > * + * {
      border-left: 1px solid rgba(217, 222, 229, 0.72);
    }

    .mobile-match-nav .file-nav-button {
      color: #78838e;
      font-weight: 650;
    }

    .mobile-match-nav button:not(:disabled):hover,
    .mobile-match-nav button:not(:disabled):focus-visible {
      background: var(--selection);
      outline: none;
    }

    .mobile-match-nav button:not(:disabled):active {
      background: var(--selection-strong);
    }

    details {
      position: relative;
    }

    summary {
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
      list-style: none;
    }

    summary::-webkit-details-marker {
      display: none;
    }

    .menu {
      position: absolute;
      top: 32px;
      right: 0;
      z-index: 5;
      display: grid;
      min-width: 150px;
      border: 1px solid var(--border);
      border-radius: 6px;
      padding: 4px;
      background: var(--panel);
      box-shadow: 0 10px 24px rgba(30, 37, 45, 0.16);
    }

    .menu button {
      height: 30px;
      border: 0;
      border-radius: 4px;
      padding: 0 8px;
      background: transparent;
      font-size: 12px;
      text-align: left;
    }

    .menu button:hover,
    .menu button:focus-visible {
      background: var(--selection);
      outline: none;
    }
  }

  @container (max-width: 620px) {
    .match-nav > .file-nav-button,
    .mobile-match-nav > .file-nav-button {
      display: none;
    }

    .file-menu-action {
      display: block;
    }
  }

  @container (max-width: 560px) {
    .panel-title {
      grid-template-columns: minmax(110px, 1fr) auto auto;
    }

    .reveal-action {
      display: none;
    }

    .reveal-menu-action {
      display: block;
    }

    .desktop-preview-actions {
      flex-wrap: nowrap;
    }
  }

  @container (max-width: 460px) {
    .panel-title {
      grid-template-columns: minmax(96px, 1fr) auto auto;
      gap: 6px;
    }

    .match-nav .match-nav-button {
      padding-inline: 7px;
    }

    .match-nav span {
      min-width: 58px;
      padding-inline: 6px;
    }
  }

  @container (max-width: 360px) {
    .panel-title {
      grid-template-columns: auto auto;
      justify-content: end;
    }

    .desktop-preview-file {
      display: none;
    }
  }

  @container (max-width: 340px) {
    .match-nav,
    .mobile-match-nav {
      grid-template-columns: auto minmax(62px, max-content) auto;
    }

    .match-nav .match-nav-button {
      padding-inline: 10px;
      font-size: 13px;
    }

    .match-nav span {
      padding-inline: 7px;
    }

    .nav-label-full {
      display: none;
    }

    .nav-label-short {
      display: inline;
    }
  }
</style>
