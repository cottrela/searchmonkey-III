<script lang="ts">
  import type { SearchMatch } from '$lib/types';

  let {
    selected,
    selectedIndex,
    total,
    query,
    regex,
    onPrevious,
    onNext
  }: {
    selected: SearchMatch | null;
    selectedIndex: number;
    total: number;
    query: string;
    regex: boolean;
    onPrevious: () => void;
    onNext: () => void;
  } = $props();

  function lineParts(text: string, term: string) {
    if (regex || !term) return [{ text, hit: false }];

    const lowerText = text.toLowerCase();
    const lowerTerm = term.toLowerCase();
    const parts: Array<{ text: string; hit: boolean }> = [];
    let cursor = 0;
    let index = lowerText.indexOf(lowerTerm);

    while (index !== -1) {
      if (index > cursor) {
        parts.push({ text: text.slice(cursor, index), hit: false });
      }

      parts.push({ text: text.slice(index, index + term.length), hit: true });
      cursor = index + term.length;
      index = lowerText.indexOf(lowerTerm, cursor);
    }

    if (cursor < text.length) {
      parts.push({ text: text.slice(cursor), hit: false });
    }

    return parts.length ? parts : [{ text, hit: false }];
  }
</script>

<aside class="preview-panel" aria-label="Match preview">
  <div class="panel-title">
    <h2>Preview</h2>
    {#if selected}
      <span>{selectedIndex + 1} / {total}</span>
    {/if}
  </div>

  {#if selected}
    <div class="preview-body">
      <div class="file" title={selected.path}>{selected.path}</div>
      <div class="editor">
        <div class="gutter">{selected.line_number}</div>
        <pre><code>{#each lineParts(selected.line_text, query) as part}{#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}</code></pre>
      </div>

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
    min-width: 0;
    border-left: 1px solid var(--border);
    background: var(--panel);
    overflow: auto;
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
    padding: 14px;
  }

  .file {
    overflow-wrap: anywhere;
    color: var(--text);
    font-size: 13px;
    font-weight: 800;
    line-height: 18px;
  }

  .editor {
    display: grid;
    grid-template-columns: 52px minmax(0, 1fr);
    margin: 12px 0 0;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: auto;
    background: #fbfcfd;
  }

  .gutter {
    border-right: 1px solid var(--border-subtle);
    padding: 12px 10px;
    color: var(--muted);
    background: #f1f4f6;
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: 12px;
    line-height: 20px;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  pre {
    margin: 0;
    padding: 12px;
    overflow: visible;
    color: var(--text);
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: 13px;
    line-height: 20px;
    white-space: pre-wrap;
  }

  mark {
    border-radius: 3px;
    padding: 0 1px;
    color: #241800;
    background: #ffd86b;
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
</style>
