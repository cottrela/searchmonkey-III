<script lang="ts">
  import { defaultSearchOptions, type SearchMatch, type SearchOptions } from '$lib/types';

  type SampleResult = {
    match: SearchMatch;
    parts: Array<{ text: string; hit: boolean }>;
  };

  let {
    query = $bindable(''),
    options = $bindable<SearchOptions>(defaultSearchOptions()),
    samples = [],
    onClose
  }: {
    query: string;
    options: SearchOptions;
    samples: SearchMatch[];
    onClose: () => void;
  } = $props();

  let pattern = $state(query);
  let updateSearchOnClose = $state(true);

  const evaluation = $derived.by(() => evaluatePattern(pattern, samples));

  function evaluatePattern(regexPattern: string, sourceMatches: SearchMatch[]) {
    if (!regexPattern.trim()) {
      return { total: 0, rows: [] as SampleResult[], error: '' };
    }

    try {
      const flags = options.case_sensitive ? 'g' : 'gi';
      const expression = new RegExp(regexPattern, flags);
      const rows: SampleResult[] = [];
      let total = 0;

      for (const match of sourceMatches) {
        const ranges = rangesForLine(expression, match.line_text);

        if (!ranges.length) continue;

        total += ranges.length;
        if (rows.length < 30) {
          rows.push({ match, parts: splitLine(match.line_text, ranges) });
        }
      }

      return { total, rows, error: '' };
    } catch (error) {
      return {
        total: 0,
        rows: [] as SampleResult[],
        error: error instanceof Error ? error.message : 'Invalid regular expression'
      };
    }
  }

  function rangesForLine(expression: RegExp, text: string) {
    const ranges: Array<{ start: number; end: number }> = [];
    expression.lastIndex = 0;
    let match: RegExpExecArray | null;

    while ((match = expression.exec(text)) && ranges.length < 20) {
      ranges.push({
        start: match.index,
        end: match.index + match[0].length
      });

      if (match[0] === '') expression.lastIndex += 1;
    }

    return ranges;
  }

  function splitLine(text: string, ranges: Array<{ start: number; end: number }>) {
    const parts: Array<{ text: string; hit: boolean }> = [];
    let cursor = 0;

    for (const range of ranges) {
      const start = Math.max(0, Math.min(range.start, text.length));
      const end = Math.max(start, Math.min(range.end, text.length));

      if (start > cursor) parts.push({ text: text.slice(cursor, start), hit: false });
      if (end > start) parts.push({ text: text.slice(start, end), hit: true });
      cursor = end;
    }

    if (cursor < text.length) parts.push({ text: text.slice(cursor), hit: false });
    return parts.length ? parts : [{ text, hit: false }];
  }

  function closeTester() {
    if (updateSearchOnClose) {
      query = pattern;
    }

    onClose();
  }

  function filename(filePath: string) {
    const parts = filePath.split('/').filter(Boolean);
    return parts.at(-1) || filePath;
  }
</script>

<aside class="regex-panel" aria-label="Regex tester">
  <header>
    <div>
      <h2>Regex Tester</h2>
      <span>Live against current result snippets</span>
    </div>
    <button type="button" onclick={closeTester}>Close</button>
  </header>

  <div class="tester-body">
    <div class="field">
      <label for="regex-test-pattern">Pattern</label>
      <input
        id="regex-test-pattern"
        type="text"
        bind:value={pattern}
        placeholder="Examples: error|warning, \bTODO\b, ^import\s+(.+)$"
        spellcheck="false"
      />
    </div>

    <div class="tester-options">
      <label>
        <input type="checkbox" bind:checked={options.case_sensitive} />
        <span>Case sensitive</span>
      </label>
      <label>
        <input type="checkbox" bind:checked={updateSearchOnClose} />
        <span>Update search input on close</span>
      </label>
    </div>

    <section class="matches" class:error={Boolean(evaluation.error)}>
      <div class="match-summary">
        {#if evaluation.error}
          <strong>{evaluation.error}</strong>
        {:else}
          <strong>{evaluation.total} matches</strong>
          <span>{samples.length ? `${evaluation.rows.length} sample lines` : 'No result snippets yet'}</span>
        {/if}
      </div>

      {#if !evaluation.error && !samples.length}
        <div class="empty">Run a regex search to test against real file snippets.</div>
      {:else if !evaluation.error && !evaluation.rows.length}
        <div class="empty">No current result snippets match this pattern.</div>
      {:else if !evaluation.error}
        <div class="sample-list">
          {#each evaluation.rows as row (`${row.match.path}:${row.match.line_number}:${row.match.line_text}`)}
            <article class="sample-row">
              <div class="sample-meta">
                <strong title={row.match.path}>{filename(row.match.path)}</strong>
                <span>Line {row.match.line_number}</span>
              </div>
              <code>
                {#each row.parts as part}
                  {#if part.hit}
                    <mark>{part.text}</mark>
                  {:else}
                    <span>{part.text}</span>
                  {/if}
                {/each}
              </code>
            </article>
          {/each}
        </div>
      {/if}
    </section>
  </div>
</aside>

<style>
  .regex-panel {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    min-width: 0;
    border-left: 1px solid var(--border);
    background: var(--panel);
    animation: slide-in 140ms ease-out;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 47px;
    border-bottom: 1px solid var(--border);
    padding: 8px 12px;
    background: var(--surface);
  }

  header div {
    display: grid;
    gap: 2px;
    min-width: 0;
  }

  h2 {
    margin: 0;
    font-size: 14px;
  }

  header span,
  label,
  .match-summary span,
  .empty {
    color: var(--muted);
    font-size: 12px;
    font-weight: 700;
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
    font-weight: 750;
    cursor: pointer;
  }

  .tester-body {
    display: grid;
    grid-template-rows: auto auto minmax(0, 1fr);
    gap: 10px;
    min-height: 0;
    overflow: auto;
    padding: 12px;
  }

  .field {
    display: grid;
    gap: 5px;
  }

  input[type='text'],
  input:not([type]) {
    width: 100%;
    min-width: 0;
    height: 34px;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    padding: 0 9px;
    color: var(--text);
    background: var(--input);
    font: inherit;
    font-size: 12px;
  }

  input:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--focus);
    outline: none;
  }

  .tester-options {
    display: grid;
    gap: 7px;
  }

  .tester-options label {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 8px;
    align-items: center;
  }

  .matches {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    gap: 9px;
    min-height: 0;
    border-top: 1px solid var(--border-subtle);
    padding-top: 10px;
  }

  .matches.error {
    color: var(--danger);
  }

  .match-summary {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
  }

  .match-summary strong {
    font-size: 13px;
  }

  .sample-list {
    display: grid;
    align-content: start;
    gap: 8px;
    min-height: 0;
    overflow: auto;
  }

  .sample-row {
    display: grid;
    gap: 4px;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    padding: 7px;
    background: var(--surface);
  }

  .sample-meta {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    min-width: 0;
    color: var(--muted);
    font-size: 11px;
    font-weight: 750;
  }

  .sample-meta strong {
    min-width: 0;
    overflow: hidden;
    color: var(--text);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  code {
    min-width: 0;
    overflow: hidden;
    color: var(--text);
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: 12px;
    line-height: 1.45;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  mark {
    border-radius: 3px;
    padding: 0 1px;
    color: #241800;
    background: #ffd86b;
  }

  .empty {
    display: grid;
    min-height: 120px;
    place-items: center;
    text-align: center;
  }

  @keyframes slide-in {
    from {
      opacity: 0.4;
      transform: translateX(18px);
    }

    to {
      opacity: 1;
      transform: translateX(0);
    }
  }
</style>
