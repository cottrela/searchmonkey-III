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
    active: boolean;
    truncated: boolean;
  };

  type RenderLine = SourceLine & {
    segments: Segment[];
  };

  const PREVIEW_LINE_LIMIT = 3000;
  const LINE_HEIGHT = 18;
  const OVERSCAN_LINES = 24;

  let {
    preview,
    errorMessage,
    total,
    query,
    regex,
    caseSensitive,
    onPrevious,
    onNext
  }: {
    preview: PreviewState;
    errorMessage: string;
    total: number;
    query: string;
    regex: boolean;
    caseSensitive: boolean;
    onPrevious: () => void;
    onNext: () => void;
  } = $props();

  let previewElement = $state<HTMLDivElement>();
  let viewportHeight = $state(0);
  let scrollTop = $state(0);
  let lastScrolledTarget = '';
  let pendingScrollFrame = 0;

  const activeResultNumber = $derived.by(() => {
    if (preview.activeMatchIndex < 0) return 0;
    return preview.matches[preview.activeMatchIndex]?.line_number ?? 0;
  });

  const sourceLines = $derived.by(() => buildSourceLines(preview.content, preview.matches, preview.activeMatchIndex));
  const totalHeight = $derived(sourceLines.length * LINE_HEIGHT);
  const visibleStart = $derived.by(() =>
    Math.max(0, Math.floor(scrollTop / LINE_HEIGHT) - OVERSCAN_LINES)
  );
  const visibleCount = $derived.by(() =>
    Math.ceil(viewportHeight / LINE_HEIGHT) + OVERSCAN_LINES * 2
  );
  const visibleEnd = $derived.by(() =>
    Math.min(sourceLines.length, visibleStart + visibleCount)
  );
  const topSpacerHeight = $derived(visibleStart * LINE_HEIGHT);
  const bottomSpacerHeight = $derived(Math.max(0, totalHeight - visibleEnd * LINE_HEIGHT));
  const visibleLines = $derived.by(() =>
    sourceLines.slice(visibleStart, visibleEnd).map((line) => ({
      ...line,
      segments: splitLine(line.text, query, regex, caseSensitive, line.active, line.truncated)
    }))
  );

  function buildSourceLines(
    content: string,
    matches: PreviewState['matches'],
    activeMatchIndex: number
  ): SourceLine[] {
    if (!content) return [];

    const allLines = content.split(/\r?\n/);
    const activeLineNumber =
      activeMatchIndex >= 0 ? matches[activeMatchIndex]?.line_number : undefined;
    const limit = Math.min(allLines.length, PREVIEW_LINE_LIMIT);
    const lines: SourceLine[] = [];

    for (let index = 0; index < limit; index += 1) {
      const number = index + 1;
      lines.push({
        number,
        text: allLines[index],
        active: activeLineNumber === number,
        truncated: false
      });
    }

    if (allLines.length > PREVIEW_LINE_LIMIT) {
      const activeIsOutsideLimit =
        typeof activeLineNumber === 'number' && activeLineNumber > PREVIEW_LINE_LIMIT;

      lines.push({
        number: PREVIEW_LINE_LIMIT + 1,
        active: activeIsOutsideLimit,
        truncated: true,
        text: activeIsOutsideLimit
          ? `Preview truncated at ${PREVIEW_LINE_LIMIT.toLocaleString()} lines. Selected match is on line ${activeLineNumber.toLocaleString()}.`
          : `Preview truncated at ${PREVIEW_LINE_LIMIT.toLocaleString()} lines.`
      });
    }

    return lines;
  }

  function splitLine(
    text: string,
    term: string,
    isRegex: boolean,
    useCaseSensitive: boolean,
    active: boolean,
    truncated: boolean
  ): Segment[] {
    if (!term || truncated) return [{ text, match: false, active }];

    const spans = isRegex ? regexSpans(text, term, useCaseSensitive) : fixedSpans(text, term, useCaseSensitive);
    if (!spans.length) return [{ text, match: false, active }];

    const segments: Segment[] = [];
    let cursor = 0;

    for (const span of spans) {
      if (span.start > cursor) {
        segments.push({ text: text.slice(cursor, span.start), match: false, active });
      }

      segments.push({
        text: text.slice(span.start, span.end),
        match: true,
        active
      });
      cursor = span.end;
    }

    if (cursor < text.length) {
      segments.push({ text: text.slice(cursor), match: false, active });
    }

    return segments.length ? segments : [{ text, match: false, active }];
  }

  function fixedSpans(text: string, term: string, useCaseSensitive: boolean) {
    const spans: Array<{ start: number; end: number }> = [];
    const haystack = useCaseSensitive ? text : text.toLowerCase();
    const needle = useCaseSensitive ? term : term.toLowerCase();
    let cursor = 0;
    let index = haystack.indexOf(needle, cursor);

    while (index !== -1) {
      spans.push({ start: index, end: index + term.length });
      cursor = index + term.length;
      index = haystack.indexOf(needle, cursor);
    }

    return spans;
  }

  function regexSpans(text: string, term: string, useCaseSensitive: boolean) {
    const spans: Array<{ start: number; end: number }> = [];

    try {
      const expression = new RegExp(term, useCaseSensitive ? 'g' : 'gi');
      let match: RegExpExecArray | null;

      while ((match = expression.exec(text)) !== null) {
        if (match[0].length === 0) {
          expression.lastIndex += 1;
          continue;
        }

        spans.push({ start: match.index, end: match.index + match[0].length });
      }
    } catch {
      return [];
    }

    return spans;
  }

  function updateScrollPosition() {
    if (pendingScrollFrame) return;

    pendingScrollFrame = requestAnimationFrame(() => {
      pendingScrollFrame = 0;
      scrollTop = previewElement?.scrollTop ?? 0;
    });
  }

  function activeLineOffset() {
    const activeIndex = sourceLines.findIndex((line) => line.active);
    if (activeIndex < 0) return null;

    return Math.max(0, activeIndex * LINE_HEIGHT - viewportHeight / 2 + LINE_HEIGHT / 2);
  }

  $effect(() => {
    const target = `${preview.filePath}:${preview.activeMatchIndex}:${preview.content.length}`;
    if (!previewElement || !sourceLines.length || target === lastScrolledTarget) return;

    lastScrolledTarget = target;

    tick().then(() => {
      const offset = activeLineOffset();
      if (offset === null || !previewElement) return;

      previewElement.scrollTop = offset;
      scrollTop = offset;
    });
  });
</script>

<aside class="preview-panel" aria-label="Match preview">
  <div class="panel-title">
    <h2>Preview</h2>
    {#if preview.filePath && activeResultNumber}
      <span>{preview.activeMatchIndex + 1} / {preview.matches.length}</span>
    {/if}
  </div>

  {#if errorMessage}
    <div class="empty">{errorMessage}</div>
  {:else if preview.filePath}
    <div class="preview-body">
      <div class="file" title={preview.filePath}>{preview.filePath}</div>
      {#if preview.content}
        <div
          bind:clientHeight={viewportHeight}
          bind:this={previewElement}
          class="preview"
          onscroll={updateScrollPosition}
        >
          <div style:height={`${topSpacerHeight}px`}></div>
          {#each visibleLines as line (line.number)}
            <div
              class:truncated={line.truncated}
              class="line"
              data-active-match={line.active ? 'true' : undefined}
              data-line={line.truncated ? '' : line.number}
            >
              <span class="source">{#each line.segments as segment}{#if segment.match}<span class:active={segment.active} class="match">{segment.text}</span>{:else}<span>{segment.text}</span>{/if}{/each}</span>
            </div>
          {/each}
          <div style:height={`${bottomSpacerHeight}px`}></div>
        </div>
      {:else}
        <div class="empty inline">Reading file...</div>
      {/if}

      <div class="nav">
        <button type="button" onclick={onPrevious} disabled={total < 2}>Previous</button>
        <button type="button" onclick={onNext} disabled={total < 2}>Next</button>
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
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 47px;
    border-bottom: 1px solid var(--border);
    padding: 0 14px;
    background: var(--panel);
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

  .preview-body {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    min-height: 0;
    padding: 14px;
  }

  .file {
    overflow: hidden;
    color: var(--text);
    font-size: 13px;
    font-weight: 800;
    line-height: 18px;
    text-overflow: ellipsis;
    white-space: nowrap;
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
    display: flex;
    min-width: max-content;
    height: 18px;
  }

  .line::before {
    content: attr(data-line);
    flex: 0 0 44px;
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

  .source {
    flex: 0 0 auto;
    padding: 0 8px;
    white-space: pre;
    user-select: text;
  }

  .line.truncated {
    min-width: 100%;
  }

  .line.truncated .source {
    color: var(--muted);
    font-family:
      Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    font-size: 12px;
    font-weight: 700;
    white-space: normal;
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

  .nav {
    display: flex;
    gap: 8px;
    margin-top: 14px;
  }

  button {
    height: 34px;
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0 10px;
    color: var(--text);
    background: var(--input);
    font: inherit;
    font-size: 13px;
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
</style>
