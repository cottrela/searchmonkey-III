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
    onNext
  }: {
    preview: PreviewState;
    errorMessage: string;
    total: number;
    onPrevious: () => void;
    onNext: () => void;
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

  $effect(() => {
    const target = preview.filePreview
      ? `${preview.filePath}:${preview.filePreview.start_line}:${preview.filePreview.end_line}:${previewTextLength}`
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
  <div class="panel-title">
    <h2>Preview</h2>
    {#if preview.filePath && activeResultNumber}
      <span>{activeResultNumber} / {total}</span>
    {/if}
  </div>

  {#if errorMessage}
    <div class="empty">{errorMessage}</div>
  {:else if preview.filePath}
    <div class="preview-body">
      <div class="file" title={preview.filePath}>{preview.filePath}</div>
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
            <div
              class="line"
              data-match={line.isMatch ? 'true' : undefined}
              data-active-match={line.isActive ? 'true' : undefined}
            >
              <span class="gutter">{line.number}</span>
              <code class="source">{#each line.segments as segment}{#if segment.match}<span class:active={segment.active} class="match">{segment.text}</span>{:else}<span>{segment.text}</span>{/if}{/each}</code>
            </div>
          {/each}
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
    grid-template-rows: auto auto auto minmax(0, 1fr) auto;
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
