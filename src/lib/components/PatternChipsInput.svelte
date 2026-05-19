<script lang="ts">
  import { tick } from 'svelte';

  let {
    id,
    label,
    placeholder = 'Add pattern...',
    values = $bindable<string[]>([]),
    examples = []
  }: {
    id: string;
    label: string;
    placeholder?: string;
    values: string[];
    examples?: string[];
  } = $props();

  let inputElement = $state<HTMLInputElement>();
  let editElement = $state<HTMLInputElement>();
  let draft = $state('');
  let selectedIndex = $state<number | null>(null);
  let editingIndex = $state<number | null>(null);
  let editDraft = $state('');

  function commitDraft() {
    const value = draft.trim();
    if (!value) return false;

    values = [...values, value];
    draft = '';
    selectedIndex = null;
    return true;
  }

  function selectChip(index: number) {
    selectedIndex = index;
    editingIndex = null;
  }

  function editChip(index: number) {
    selectedIndex = null;
    editingIndex = index;
    editDraft = values[index] ?? '';

    void tick().then(() => {
      editElement?.focus();
      editElement?.select();
    });
  }

  function saveEdit() {
    if (editingIndex === null) return;

    const value = editDraft.trim();
    const index = editingIndex;
    editingIndex = null;
    editDraft = '';

    if (!value) {
      removeChip(index);
      return;
    }

    values = values.map((item, itemIndex) => (itemIndex === index ? value : item));
  }

  function cancelEdit() {
    editingIndex = null;
    editDraft = '';
  }

  function removeChip(index: number) {
    values = values.filter((_, itemIndex) => itemIndex !== index);
    selectedIndex = null;
    editingIndex = null;

    void tick().then(() => inputElement?.focus());
  }

  function handleInputKeydown(event: KeyboardEvent) {
    if (event.key === 'Enter') {
      event.preventDefault();
      commitDraft();
      return;
    }

    if (event.key === 'Tab') {
      commitDraft();
      return;
    }

    if (event.key === 'Backspace' && !draft) {
      event.preventDefault();
      if (selectedIndex === null) {
        selectedIndex = values.length ? values.length - 1 : null;
      } else {
        removeChip(selectedIndex);
      }
      return;
    }

    if (event.key === 'Delete' && selectedIndex !== null) {
      event.preventDefault();
      removeChip(selectedIndex);
      return;
    }

    if (event.key === 'Escape') {
      selectedIndex = null;
    }
  }

  function handleEditKeydown(event: KeyboardEvent) {
    if (event.key === 'Enter') {
      event.preventDefault();
      saveEdit();
      return;
    }

    if (event.key === 'Escape') {
      event.preventDefault();
      cancelEdit();
    }
  }

  function handlePaste(event: ClipboardEvent) {
    const text = event.clipboardData?.getData('text');
    if (!text || !text.includes('\n')) return;

    event.preventDefault();
    const pastedValues = text
      .split(/\r?\n/)
      .map((item) => item.trim())
      .filter(Boolean);

    if (!pastedValues.length) return;
    values = [...values, ...pastedValues];
    draft = '';
  }
</script>

<div class="field">
  <label for={id}>{label}</label>
  <div class="chips-input">
    {#each values as value, index (`${value}-${index}`)}
      {#if editingIndex === index}
        <input
          class="chip-edit"
          bind:this={editElement}
          bind:value={editDraft}
          onkeydown={handleEditKeydown}
          onblur={saveEdit}
          aria-label={`Edit ${label.toLowerCase()} pattern`}
          spellcheck="false"
        />
      {:else}
        <span class:selected={selectedIndex === index} class="chip" title={value}>
          <button
            class="chip-value"
            type="button"
            onclick={(event) => {
              event.stopPropagation();
              selectChip(index);
            }}
            ondblclick={(event) => {
              event.stopPropagation();
              editChip(index);
            }}
          >
            {value}
          </button>
          <button
            class="remove"
            type="button"
            aria-label={`Remove ${value}`}
            onclick={(event) => {
              event.stopPropagation();
              removeChip(index);
            }}
          >
            ×
          </button>
        </span>
      {/if}
    {/each}

    <input
      {id}
      class="chip-entry"
      bind:this={inputElement}
      bind:value={draft}
      {placeholder}
      onblur={commitDraft}
      onkeydown={handleInputKeydown}
      onpaste={handlePaste}
      onfocus={() => (selectedIndex = null)}
      spellcheck="false"
    />
  </div>
  <div class="hint">
    <span>Press Enter to add. Spaces are allowed.</span>
    {#if examples.length}
      <span>{examples.join('  ')}</span>
    {/if}
  </div>
</div>

<style>
  .field {
    display: grid;
    gap: 4px;
    margin-bottom: 9px;
  }

  label {
    color: var(--muted);
    font-size: 11px;
    font-weight: 650;
  }

  .chips-input {
    display: flex;
    width: 100%;
    min-height: 34px;
    min-width: 0;
    flex-wrap: wrap;
    align-items: center;
    gap: 5px;
    box-sizing: border-box;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    padding: 4px;
    background: var(--input);
    cursor: text;
  }

  .chips-input:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--focus);
  }

  .chip {
    display: inline-flex;
    max-width: 100%;
    min-width: 0;
    height: 24px;
    align-items: center;
    gap: 5px;
    border: 1px solid var(--border-subtle);
    border-radius: 5px;
    padding: 0 5px 0 8px;
    color: var(--text);
    background: #eef3f7;
    cursor: default;
  }

  .chip.selected {
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--focus);
  }

  .chip-value {
    min-width: 0;
    border: 0;
    padding: 0;
    overflow: hidden;
    color: inherit;
    background: transparent;
    font: inherit;
    font-size: 12px;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: default;
  }

  .remove {
    display: grid;
    width: 16px;
    height: 16px;
    flex: 0 0 auto;
    place-items: center;
    border: 0;
    border-radius: 4px;
    color: var(--muted);
    background: transparent;
    font: inherit;
    font-size: 13px;
    cursor: pointer;
    line-height: 1;
  }

  .remove:hover {
    color: var(--text);
    background: var(--border-subtle);
  }

  .chip-entry,
  .chip-edit {
    min-width: 120px;
    flex: 1 1 130px;
    border: 0;
    color: var(--text);
    background: transparent;
    font: inherit;
    font-size: 12px;
    outline: none;
  }

  .chip-edit {
    height: 24px;
    flex: 1 1 180px;
    border-radius: 4px;
    background: #fff;
    padding: 0 5px;
  }

  .hint {
    display: grid;
    gap: 2px;
    color: var(--muted);
    font-size: 10px;
    line-height: 1.35;
  }
</style>
