<script lang="ts">
  import PatternChipsInput from '$lib/components/PatternChipsInput.svelte';
  import { defaultSearchOptions, type SearchOptions } from '$lib/types';

  let {
    includePatterns = $bindable<string[]>([]),
    excludePatterns = $bindable<string[]>([]),
    contextLines = $bindable(0),
    options = $bindable<SearchOptions>(defaultSearchOptions())
  }: {
    includePatterns: string[];
    excludePatterns: string[];
    contextLines: number;
    options: SearchOptions;
  } = $props();

  let advancedOpen = $state(false);

  function setSearchMode(mode: SearchOptions['search_mode']) {
    options.search_mode = mode;
    options.regex = mode === 'regex';
  }

  function applyModifiedPreset() {
    if (options.modified_preset === 'any') {
      options.modified_after = null;
      return;
    }

    const days =
      options.modified_preset === '24h'
        ? 1
        : options.modified_preset === '7d'
          ? 7
          : options.modified_preset === '30d'
            ? 30
            : Math.max(1, options.modified_custom_days || 1);

    options.modified_after = Math.floor((Date.now() - days * 24 * 60 * 60 * 1000) / 1000);
  }

  $effect(() => {
    if (options.search_mode === 'regex' !== options.regex) {
      options.regex = options.search_mode === 'regex';
    }
  });

  $effect(() => {
    contextLines = options.context_lines;
  });
</script>

<aside class="scope-panel" aria-label="Search scope">
  <div class="panel-header">
    <h2>Filters</h2>
  </div>

  <PatternChipsInput
    id="include-patterns"
    label="Include"
    placeholder="Add pattern..."
    bind:values={includePatterns}
    examples={['*.txt', '*.md', 'src/**/*.rs', 'Project Notes/*.txt']}
  />

  <PatternChipsInput
    id="exclude-patterns"
    label="Exclude"
    placeholder="Add exclusion..."
    bind:values={excludePatterns}
    examples={['node_modules', 'target', '*.tmp', 'build output']}
  />

  <button
    class="advanced-toggle"
    type="button"
    aria-expanded={advancedOpen}
    onclick={() => (advancedOpen = !advancedOpen)}
  >
    <span>{advancedOpen ? 'Hide' : 'Show'} Advanced</span>
    <span aria-hidden="true">{advancedOpen ? '−' : '+'}</span>
  </button>

  {#if advancedOpen}
    <div class="advanced">
      <section class="advanced-section">
        <h3>Search behaviour</h3>
        <div class="radio-group" aria-label="Search mode">
          <label><input type="radio" checked={options.search_mode === 'literal'} onchange={() => setSearchMode('literal')} /> Literal</label>
          <label><input type="radio" checked={options.search_mode === 'regex'} onchange={() => setSearchMode('regex')} /> Regex</label>
        </div>
        <label class="check-row">
          <input type="checkbox" bind:checked={options.multiline} />
          <span>Multiline</span>
        </label>
        <label class="check-row">
          <input type="checkbox" bind:checked={options.follow_symlinks} />
          <span>Follow symlinks</span>
        </label>
        <div class="field">
          <label for="context-lines">Context lines</label>
          <input id="context-lines" type="number" min="0" max="20" bind:value={options.context_lines} />
        </div>
      </section>

      <section class="advanced-section">
        <h3>File filters</h3>
        <div class="two-fields">
          <div class="field">
            <label for="min-file-size">Min size</label>
            <input id="min-file-size" bind:value={options.min_file_size} placeholder="0" spellcheck="false" />
          </div>
          <div class="field">
            <label for="max-file-size">Max size</label>
            <input id="max-file-size" bind:value={options.max_file_size} placeholder="10M" spellcheck="false" />
          </div>
        </div>
        <div class="field">
          <label for="modified-preset">Modified</label>
          <select id="modified-preset" bind:value={options.modified_preset} onchange={applyModifiedPreset}>
            <option value="any">Any time</option>
            <option value="24h">Last 24h</option>
            <option value="7d">Last 7d</option>
            <option value="30d">Last 30d</option>
            <option value="custom">Custom days</option>
          </select>
        </div>
        {#if options.modified_preset === 'custom'}
          <div class="field">
            <label for="modified-days">Custom days</label>
            <input id="modified-days" type="number" min="1" bind:value={options.modified_custom_days} onchange={applyModifiedPreset} />
          </div>
        {/if}
        <div class="field">
          <label for="file-type">File type</label>
          <select id="file-type" bind:value={options.file_type}>
            <option value="all">All files</option>
            <option value="text">Text</option>
            <option value="code">Code</option>
            <option value="logs">Logs</option>
            <option value="custom">Custom MIME or glob</option>
          </select>
        </div>
        {#if options.file_type === 'custom'}
          <div class="field">
            <label for="custom-file-type">Custom type</label>
            <input id="custom-file-type" bind:value={options.custom_file_type} placeholder="*.json, text/*" spellcheck="false" />
          </div>
        {/if}
      </section>

      <section class="advanced-section">
        <h3>Performance</h3>
        <label class="check-row">
          <input type="checkbox" bind:checked={options.skip_binary} />
          <span>Skip binary files</span>
        </label>
        <label class="check-row">
          <input type="checkbox" bind:checked={options.respect_gitignore} />
          <span>Use .gitignore</span>
        </label>
        <label class="check-row">
          <input type="checkbox" bind:checked={options.ignore_node_modules} />
          <span>Exclude node_modules</span>
        </label>
        <label class="check-row">
          <input type="checkbox" bind:checked={options.ignore_build_artifacts} />
          <span>Exclude build artifacts</span>
        </label>
        <div class="field">
          <label for="encoding">Encoding</label>
          <select id="encoding" bind:value={options.encoding}>
            <option value="auto">Auto</option>
            <option value="utf-8">UTF-8</option>
            <option value="windows-1250">Central European (Windows-1250)</option>
            <option value="ascii">ASCII</option>
          </select>
        </div>
        <div class="field">
          <label for="max-matches">Max matches</label>
          <input id="max-matches" type="number" min="1" max="100000" bind:value={options.max_matches} />
        </div>
      </section>

    </div>
  {/if}
</aside>

<style>
  .scope-panel {
    min-width: 0;
    border-right: 1px solid var(--border);
    background: #f7f9fb;
    padding: 10px;
    overflow: auto;
  }

  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 10px;
  }

  h2 {
    margin: 0;
    font-size: 13px;
    letter-spacing: 0;
  }

  .field {
    display: grid;
    gap: 4px;
    margin-bottom: 9px;
  }

  label,
  .check-row span {
    color: var(--muted);
    font-size: 11px;
    font-weight: 650;
  }

  input,
  select {
    width: 100%;
    min-width: 0;
    box-sizing: border-box;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    color: var(--text);
    background: var(--input);
    font: inherit;
    font-size: 12px;
    height: 32px;
    padding: 0 9px;
  }

  input:focus,
  select:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--focus);
    outline: none;
  }

  input:disabled {
    color: var(--muted);
    background: var(--disabled);
  }

  .advanced-toggle {
    display: flex;
    width: 100%;
    height: 32px;
    align-items: center;
    justify-content: space-between;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    padding: 0 10px;
    color: var(--text);
    background: var(--input);
    font: inherit;
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
  }

  .advanced {
    margin-top: 10px;
    padding-top: 10px;
    border-top: 1px solid var(--border-subtle);
  }

  .advanced-section {
    display: grid;
    gap: 8px;
    border-bottom: 1px solid var(--border-subtle);
    padding-bottom: 10px;
    margin-bottom: 10px;
  }

  h3 {
    margin: 0;
    color: var(--text);
    font-size: 12px;
    font-weight: 800;
  }

  .check-row {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 8px;
    align-items: center;
  }

  .check-row input {
    width: auto;
    height: auto;
  }

  .radio-group {
    display: grid;
    gap: 7px;
  }

  .radio-group label {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 8px;
    align-items: center;
  }

  .radio-group input {
    width: auto;
    height: auto;
  }

  .two-fields {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 8px;
  }

</style>
