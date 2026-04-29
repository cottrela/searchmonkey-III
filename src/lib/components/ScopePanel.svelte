<script lang="ts">
  let {
    path = $bindable(''),
    includePatterns = $bindable(''),
    excludePatterns = $bindable(''),
    contextLines = $bindable(0)
  }: {
    path: string;
    includePatterns: string;
    excludePatterns: string;
    contextLines: number;
  } = $props();

  let advancedOpen = $state(false);
  let followSymlinks = $state(false);
  let multiline = $state(false);
  let rawFlags = $state('');
</script>

<aside class="scope-panel" aria-label="Search scope">
  <div class="panel-header">
    <h2>Scope</h2>
  </div>

  <div class="field">
    <label for="search-path">Folder or path</label>
    <input id="search-path" bind:value={path} placeholder="/Users/name/project" spellcheck="false" />
  </div>

  <div class="field">
    <label for="include-patterns">Include</label>
    <input
      id="include-patterns"
      bind:value={includePatterns}
      placeholder="*.txt, *.log, src/**/*.rs"
      spellcheck="false"
    />
  </div>

  <div class="field">
    <label for="exclude-patterns">Exclude</label>
    <input
      id="exclude-patterns"
      bind:value={excludePatterns}
      placeholder="node_modules, target, *.tmp"
      spellcheck="false"
    />
  </div>

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
      <label class="check-row">
        <input type="checkbox" bind:checked={followSymlinks} disabled />
        <span>Follow symlinks</span>
        <em>Future</em>
      </label>

      <label class="check-row">
        <input type="checkbox" bind:checked={multiline} disabled />
        <span>Multiline</span>
        <em>Future</em>
      </label>

      <div class="field">
        <label for="context-lines">Context lines</label>
        <input id="context-lines" type="number" min="0" max="20" bind:value={contextLines} disabled />
      </div>

      <div class="field">
        <label for="raw-flags">Raw rg flags</label>
        <input id="raw-flags" bind:value={rawFlags} placeholder="Future" disabled />
      </div>
    </div>
  {/if}
</aside>

<style>
  .scope-panel {
    min-width: 0;
    border-right: 1px solid var(--border-subtle);
    background: #fbfcfd;
    padding: 12px;
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

  input {
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

  input:focus {
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

  .check-row {
    display: grid;
    grid-template-columns: auto 1fr auto;
    gap: 8px;
    align-items: center;
    margin-bottom: 10px;
  }

  .check-row input {
    width: auto;
    height: auto;
  }

  em {
    border-radius: 999px;
    padding: 1px 7px;
    color: var(--muted);
    background: var(--disabled);
    font-size: 11px;
    font-style: normal;
    font-weight: 700;
  }
</style>
