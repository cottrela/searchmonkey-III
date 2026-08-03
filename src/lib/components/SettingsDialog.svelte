<script lang="ts">
  import { onMount } from 'svelte';
  import { open } from '@tauri-apps/plugin-dialog';
  import {
    binaryFromTemplate,
    expandCommandTemplate,
    normalizeExtension,
    parseCommandTemplate,
    quoteTemplateToken,
    type FileOpenersConfig,
    type FileOpenerRule
  } from '$lib/file-openers';
  import { validateFileOpenerCommand } from '$lib/search';

  let {
    config,
    onClose,
    onChanged
  }: {
    config: FileOpenersConfig;
    onClose: () => void;
    onChanged: (config: FileOpenersConfig) => void;
  } = $props();

  let rules = $state<FileOpenerRule[]>([]);
  let editingIndex = $state<number | null>(null);
  let extension = $state('');
  let template = $state('');
  let error = $state('');

  const samplePath = $derived(`/example/search-result.${normalizeExtension(extension) || 'txt'}`);
  const preview = $derived(expandCommandTemplate(template, samplePath));

  onMount(() => {
    rules = config.rules.map((rule) => ({ ...rule }));
  });

  function beginAdd() {
    editingIndex = -1;
    extension = '';
    template = '';
    error = '';
  }

  function beginEdit(index: number) {
    editingIndex = index;
    extension = rules[index].extension;
    template = rules[index].template;
    error = '';
  }

  function cancelEdit() {
    editingIndex = null;
    error = '';
  }

  async function chooseBinary() {
    const selected = await open({ multiple: false, directory: false, title: 'Choose application or executable' });
    if (typeof selected !== 'string') return;
    const existingArguments = template.trim() ? template.trim().replace(/^\s*(?:"[^"]*"|'[^']*'|\S+)/, '').trim() : '';
    template = `${quoteTemplateToken(selected)}${existingArguments ? ` ${existingArguments}` : ' {path}'}`;
  }

  async function saveEntry() {
    const normalized = normalizeExtension(extension);
    if (!normalized) {
      error = 'Enter one file extension.';
      return;
    }
    if (!template.trim().includes('{path}')) {
      error = 'The command template must include {path}.';
      return;
    }
    const parsed = parseCommandTemplate(template);
    if (!parsed) {
      error = 'Enter an application or executable.';
      return;
    }
    try {
      await validateFileOpenerCommand(parsed.command);
    } catch (validationError) {
      error = validationError instanceof Error ? validationError.message : String(validationError);
      return;
    }
    const duplicate = rules.findIndex((rule, index) => rule.extension === normalized && index !== editingIndex);
    if (duplicate >= 0) {
      error = `An override for .${normalized} already exists.`;
      return;
    }

    const rule = { extension: normalized, template: template.trim() };
    if (editingIndex === -1) rules.push(rule);
    else if (editingIndex !== null) rules[editingIndex] = rule;
    editingIndex = null;
    error = '';
    onChanged({ rules: [...rules] });
  }

  function removeEntry(index: number) {
    rules.splice(index, 1);
    if (editingIndex === index) cancelEdit();
    else if (editingIndex !== null && editingIndex > index) editingIndex -= 1;
    onChanged({ rules: [...rules] });
  }
</script>

<div class="settings" role="dialog" aria-modal="true" aria-labelledby="settings-title">
  <header class="settings-header">
    <div>
      <h1 id="settings-title">Settings</h1>
      <p>Configure how Searchmonkey behaves.</p>
    </div>
    <button type="button" onclick={onClose}>Done</button>
  </header>

  <main>
    <section class="settings-section" aria-labelledby="file-opening-title">
      <div class="section-heading">
        <div>
          <h2 id="file-opening-title">File opening</h2>
          <p>Override the system-default application for individual file extensions.</p>
        </div>
      </div>

      <div class="entries">
        {#each rules as rule, index}
          <button
            class="entry"
            class:active={editingIndex === index}
            type="button"
            aria-expanded={editingIndex === index}
            onclick={() => editingIndex === index ? cancelEdit() : beginEdit(index)}
          >
            <span class="extension"><code>.{rule.extension}</code></span>
            <span class="entry-detail">
              <strong>{binaryFromTemplate(rule.template)}</strong>
              <small>{rule.template}</small>
            </span>
            <span class="chevron" aria-hidden="true">{editingIndex === index ? '⌃' : '›'}</span>
          </button>

          {#if editingIndex === index}
            {@render editor()}
          {/if}
        {/each}

        <button class="add-entry" class:active={editingIndex === -1} type="button" onclick={beginAdd}>
          <span class="add-icon" aria-hidden="true">+</span>
          <span><strong>Add extension</strong><small>Create another file-opening override</small></span>
        </button>

        {#if editingIndex === -1}
          {@render editor()}
        {/if}
      </div>
    </section>
  </main>
</div>

{#snippet editor()}
  <form class="editor" onsubmit={(event) => { event.preventDefault(); void saveEntry(); }}>
    <div class="editor-grid">
      <label>
        <span>Extension</span>
        <div class="extension-input"><span>.</span><input bind:value={extension} placeholder="txt" aria-label="File extension" /></div>
      </label>
      <label class="template-field">
        <span>Command template</span>
        <div class="template-input">
          <input bind:value={template} placeholder={'/my/bin {path} --line {line}'} />
          <button type="button" onclick={chooseBinary}>Browse…</button>
        </div>
      </label>
    </div>

    <div class="examples">
      <p><strong>Placeholders:</strong> <code>{'{path}'}</code>, <code>{'{line}'}</code>, <code>{'{column}'}</code></p>
      <p><strong>Example:</strong> <code>code --goto {'{path}'}:{'{line}'}:{'{column}'}</code></p>
      <div class="preview">
        <span>Preview for <code>{samplePath}</code>, line 42:</span>
        <code>{preview || 'Enter a command template to see its preview.'}</code>
      </div>
    </div>

    {#if error}<p class="error">{error}</p>{/if}
    <div class="editor-actions">
      {#if editingIndex !== null && editingIndex >= 0}
        <button class="remove" type="button" onclick={() => removeEntry(editingIndex!)}>Remove override</button>
        <span class="action-spacer"></span>
      {/if}
      <button type="button" onclick={cancelEdit}>Cancel</button>
      <button class="primary" type="submit">{editingIndex === -1 ? 'Add override' : 'Save changes'}</button>
    </div>
  </form>
{/snippet}

<style>
  .settings { position: fixed; inset: 0; z-index: 48; display: grid; grid-template-rows: auto minmax(0, 1fr); width: 100vw; height: 100dvh; overflow: hidden; color: var(--text); background: var(--panel); }
  .settings-header { z-index: 2; display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 18px clamp(18px, 4vw, 52px); border-bottom: 1px solid var(--border); background: var(--surface); }
  h1, h2, p { margin: 0; }
  h1 { font-size: 22px; }
  h2 { font-size: 17px; }
  header p, .section-heading p { margin-top: 4px; color: var(--muted); font-size: 12px; }
  main { box-sizing: border-box; width: 100%; min-height: 0; overflow-y: auto; overscroll-behavior: contain; padding: 34px max(18px, calc((100% - 960px) / 2)) 80px; }
  .settings-section { display: grid; gap: 18px; }
  .section-heading { display: flex; align-items: end; justify-content: space-between; gap: 16px; }
  .entries { overflow: hidden; border: 1px solid var(--border); border-radius: 8px; background: var(--surface); }
  .entry { display: grid; grid-template-columns: minmax(100px, .3fr) minmax(180px, 1fr) auto; align-items: center; gap: 14px; width: 100%; min-height: 64px; padding: 10px 14px; border: 0; border-top: 1px solid var(--border); border-radius: 0; text-align: left; }
  .entry:first-child { border-top: 0; }
  .entry:hover, .entry.active, .add-entry:hover, .add-entry.active { background: color-mix(in srgb, var(--accent) 7%, var(--surface)); }
  .extension code { display: inline-block; min-width: 52px; padding: 5px 8px; border-radius: 5px; color: var(--text); background: var(--panel); font-weight: 700; }
  .entry-detail { display: grid; min-width: 0; gap: 3px; }
  .entry-detail strong, .entry-detail small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .entry-detail small, .add-entry small { color: var(--muted); font-size: 11px; font-weight: 400; }
  .chevron { color: var(--muted); font-size: 20px; }
  .add-entry { display: flex; align-items: center; gap: 12px; width: 100%; min-height: 60px; padding: 10px 14px; border: 0; border-top: 1px solid var(--border); border-radius: 0; text-align: left; }
  .add-entry > span:last-child { display: grid; gap: 2px; }
  .add-icon { display: grid; width: 28px; height: 28px; place-items: center; border: 1px solid var(--border); border-radius: 50%; color: var(--accent); font-size: 20px; }
  .editor { display: grid; gap: 14px; padding: 16px; border-top: 1px solid var(--border); background: var(--panel); }
  .editor-grid { display: grid; grid-template-columns: minmax(110px, .32fr) minmax(300px, 1fr); gap: 12px; }
  label { display: grid; align-content: start; gap: 6px; font-size: 12px; font-weight: 600; }
  input { box-sizing: border-box; min-width: 0; width: 100%; border: 1px solid var(--border); border-radius: 4px; padding: 8px; color: var(--text); background: var(--surface); font: inherit; }
  .extension-input, .template-input { display: flex; align-items: center; gap: 7px; }
  .template-input input { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
  button { border: 1px solid var(--border); border-radius: 4px; padding: 6px 10px; color: var(--text); background: var(--surface); white-space: nowrap; }
  .primary { color: white; border-color: var(--accent); background: var(--accent); }
  .examples { display: grid; gap: 7px; color: var(--muted); font-size: 12px; }
  .preview { display: grid; gap: 5px; padding: 10px; border: 1px solid var(--border); border-radius: 5px; background: var(--surface); }
  .preview > code { overflow-wrap: anywhere; color: var(--text); }
  .error { color: var(--danger, #b42318); font-size: 12px; }
  .editor-actions { display: flex; justify-content: flex-end; gap: 8px; }
  .action-spacer { flex: 1; }
  .remove { color: var(--danger, #b42318); }
  code { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
  @media (max-width: 650px) {
    .entry { grid-template-columns: 72px minmax(0, 1fr) auto; }
    .editor-grid { grid-template-columns: 1fr; }
    .section-heading { align-items: start; }
  }
</style>
