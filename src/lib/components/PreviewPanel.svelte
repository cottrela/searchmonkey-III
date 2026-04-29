<script lang="ts">
  import { tick } from 'svelte';
  import type { PreviewState } from '$lib/types';

  type Segment = {
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
  };

  type RenderLine = SourceLine & {
    segments: Segment[];
  };

  const WRAP_LIMIT = 50_000;

  let {
    preview,
    errorMessage,
    total,
    onPrevious,
    onNext,
    onSelect,
    onOpen,
    onReveal,
    onClose
  }: {
    preview: PreviewState;
    errorMessage: string;
    total: number;
    onPrevious: () => void;
    onNext: () => void;
    onSelect: (match: PreviewState['matches'][number]) => void;
    onOpen: (path: string) => void;
    onReveal: (path: string) => void;
    onClose: () => void;
  } = $props();

  let previewElement = $state<HTMLDivElement>();
  let lastScrolledTarget = '';
  let wrapLines = $state(false);

  const activeResultNumber = $derived.by(() => {
    if (preview.activeMatchIndex < 0) return 0;
    return preview.activeMatchIndex + 1;
  });

  const sourceLines = $derived.by(() =>
    buildSourceLines(preview.filePreview, preview.matches, preview.activeMatch)
  );
  const previewTextLength = $derived.by(() =>
    sourceLines.reduce((length, line) => length + line.text.length + 1, 0)
  );
  const canWrap = $derived(previewTextLength < WRAP_LIMIT);
  const effectiveWrap = $derived(wrapLines && canWrap);
  const renderLines = $derived.by(() =>
    sourceLines.map((line) => ({
      ...line,
      segments: splitLine(line.text, line.matchRanges, line.isActive)
    }))
  );
  const activeMatchText = $derived(preview.activeMatch?.line_text ?? '');
  const activeMatchOnly = $derived.by(() => {
    const match = preview.activeMatch;
    if (!match?.submatches.length) return activeMatchText;
    return match.submatches.map((range) => match.line_text.slice(range.start, range.end)).join(' ');
  });

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
        matchRanges: mergeMatchRanges(matchesByLine.get(line.number) ?? [])
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

  function splitLine(
    text: string,
    matchRanges: Array<{ start: number; end: number }>,
    active: boolean
  ): Segment[] {
    if (!matchRanges.length) return [{ text, match: false, active }];

    const segments: Segment[] = [];
    let cursor = 0;

    for (const range of matchRanges) {
      const start = Math.max(0, Math.min(range.start, text.length));
      const end = Math.max(start, Math.min(range.end, text.length));

      if (end <= cursor) {
        continue;
      }

      if (start > cursor) {
        segments.push({ text: text.slice(cursor, start), match: false, active });
      }

      segments.push({
        text: text.slice(Math.max(start, cursor), end),
        match: true,
        active
      });
      cursor = end;
    }

    if (cursor < text.length) {
      segments.push({ text: text.slice(cursor), match: false, active });
    }

    return segments.length ? segments : [{ text, match: false, active }];
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

<aside class="preview-panel" aria-label="Match preview">
  {#if preview.filePath}
    <div class="mobile-preview-toolbar">
      <button type="button" onclick={onClose} title="Back to results">← Results</button>
      <span></span>
      <button type="button" onclick={() => onOpen(preview.filePath)} title="Open file">Open</button>
      <details>
        <summary title="More actions" aria-label="More actions">...</summary>
        <div class="menu">
          <button type="button" onclick={() => onReveal(preview.filePath)}>Reveal</button>
          <button type="button" onclick={() => copyText(activeMatchOnly)} disabled={!activeMatchOnly}>
            Copy match
          </button>
          <button type="button" onclick={() => copyText(activeMatchText)} disabled={!activeMatchText}>
            Copy line
          </button>
          <button type="button" onclick={() => copyText(preview.filePath)}>Copy path</button>
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
          {#if activeResultNumber}
            <span>{activeResultNumber} / {total}</span>
          {/if}
        </div>
        <div class="desktop-preview-path" title={parentPath(preview.filePath)}>
          {parentPath(preview.filePath)}
        </div>
      </div>
      <div class="desktop-preview-actions">
        <div class="match-nav" aria-label="Match navigation">
          <button type="button" onclick={onPrevious} disabled={total < 2} title="Previous match">←</button>
          <span>{activeResultNumber} / {total}</span>
          <button type="button" onclick={onNext} disabled={total < 2} title="Next match">→</button>
        </div>
        <button type="button" onclick={() => onOpen(preview.filePath)} title="Open file">Open</button>
        <button type="button" onclick={() => onReveal(preview.filePath)} title="Reveal file">Reveal</button>
        <details>
          <summary title="More actions" aria-label="More actions">...</summary>
          <div class="menu">
            <button type="button" onclick={() => copyText(activeMatchOnly)} disabled={!activeMatchOnly}>
              Copy match
            </button>
            <button type="button" onclick={() => copyText(preview.filePath)}>Copy path</button>
            <button type="button" disabled>Open with...</button>
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
      <label class="wrap-toggle">
        <input type="checkbox" bind:checked={wrapLines} disabled={!canWrap} />
        <span>Wrap lines</span>
      </label>
      {#if !canWrap}
        <div class="wrap-message">Line wrapping is disabled for previews over 50,000 characters.</div>
      {/if}
      {#if preview.filePreview}
        <div
          bind:this={previewElement}
          class="preview"
          class:wrap={effectiveWrap}
        >
          {#each renderLines as line (line.number)}
            {#if line.isMatch}
              <div
                class="line"
                role="button"
                tabindex="0"
                data-match="true"
                data-active-match={line.isActive ? 'true' : undefined}
                onclick={() => selectLineMatch(line.number)}
                onkeydown={(event) => handleLineKeydown(event, line.number)}
              >
                <span class="gutter">{line.number}</span>
                <code class="source">{#each line.segments as segment}{#if segment.match}<span class:active={segment.active} class="match">{segment.text}</span>{:else}<span>{segment.text}</span>{/if}{/each}</code>
              </div>
            {:else}
              <div class="line">
                <span class="gutter">{line.number}</span>
                <code class="source">{#each line.segments as segment}{#if segment.match}<span class:active={segment.active} class="match">{segment.text}</span>{:else}<span>{segment.text}</span>{/if}{/each}</code>
              </div>
            {/if}
          {/each}
        </div>
      {:else}
        <div class="empty inline">Reading file...</div>
      {/if}

      <div class="mobile-match-nav">
        <button type="button" onclick={onPrevious} disabled={total < 2} title="Previous match">←</button>
        <span>{activeResultNumber} / {total}</span>
        <button type="button" onclick={onNext} disabled={total < 2} title="Next match">→</button>
      </div>

    </div>
  {:else}
    <div class="empty">Select a match to preview it</div>
  {/if}
</aside>

<style>
  .preview-panel {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    min-width: 0;
    min-height: 0;
    background: var(--panel);
    overflow: hidden;
  }

  .panel-title {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 10px;
    align-items: center;
    min-height: 64px;
    border-bottom: 1px solid var(--border);
    padding: 7px 10px 7px 14px;
    background: var(--panel);
  }

  .desktop-preview-file {
    display: grid;
    gap: 2px;
    min-width: 0;
  }

  .desktop-preview-title {
    display: flex;
    min-width: 0;
    align-items: baseline;
    gap: 10px;
  }

  h2 {
    min-width: 0;
    margin: 0;
    font-size: 14px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .desktop-preview-title span {
    flex: 0 0 auto;
    color: var(--muted);
    font-size: 12px;
    font-weight: 700;
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
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 6px;
  }

  .match-nav {
    display: grid;
    grid-template-columns: 30px minmax(58px, auto) 30px;
    align-items: center;
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--input);
  }

  .match-nav button {
    border: 0;
    border-radius: 0;
    background: transparent;
  }

  .match-nav span {
    color: var(--muted);
    font-size: 12px;
    font-weight: 800;
    text-align: center;
    white-space: nowrap;
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
    background: var(--panel);
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

  .preview-body {
    display: grid;
    grid-template-rows: auto auto minmax(0, 1fr);
    min-height: 0;
    padding: 14px;
  }

  .wrap-toggle {
    display: flex;
    align-items: center;
    gap: 7px;
    margin-top: 10px;
    color: var(--text);
    font-size: 12px;
    font-weight: 700;
  }

  .wrap-toggle input {
    margin: 0;
  }

  .wrap-message {
    margin-top: 6px;
    color: var(--muted);
    font-size: 12px;
    line-height: 16px;
  }

  .preview {
    margin: 12px 0 0;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: auto;
    color: var(--text);
    background: #fbfcfd;
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: 13px;
    line-height: 18px;
    contain: content;
  }

  .line {
    display: grid;
    grid-template-columns: 44px minmax(0, 1fr);
    min-width: max-content;
    min-height: 18px;
  }

  .line[data-match='true'] {
    cursor: pointer;
  }

  .gutter {
    border-right: 1px solid var(--border-subtle);
    padding: 0 6px 0 4px;
    color: var(--muted);
    background: #f1f4f6;
    text-align: right;
    font-variant-numeric: tabular-nums;
    pointer-events: none;
    user-select: none;
  }

  .line[data-active-match='true'] {
    background: #fff4cf;
    outline: 1px solid #c88f16;
    outline-offset: -1px;
  }

  .line[data-match='true']:not([data-active-match='true']) {
    background: #fff9e8;
  }

  .source {
    padding: 0 8px;
    overflow-wrap: normal;
    white-space: pre;
    word-break: normal;
    user-select: text;
  }

  .preview.wrap .line {
    min-width: 0;
  }

  .preview.wrap .source {
    min-width: 0;
    white-space: pre-wrap;
  }

  .match {
    border-radius: 3px;
    padding: 0 1px;
    color: #241800;
    background: #ffd86b;
  }

  .match.active {
    outline: 1px solid #b06b00;
    background: #ffbd3d;
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
      grid-template-columns: minmax(0, 1fr);
    }

    .desktop-preview-actions {
      justify-content: flex-start;
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
      grid-template-rows: minmax(0, 1fr) auto;
      padding: 8px;
    }

    .wrap-toggle,
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
      grid-template-columns: 42px minmax(0, 1fr) 42px;
      gap: 8px;
      align-items: center;
      margin-top: 8px;
    }

    .mobile-match-nav button {
      height: 34px;
      font-size: 16px;
    }

    .mobile-match-nav span {
      color: var(--muted);
      font-size: 12px;
      font-weight: 800;
      text-align: center;
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
</style>
