<script lang="ts">
  let { onClose }: { onClose?: () => void } = $props();

  const sections = [
    {
      title: 'Characters',
      rows: [
        ['.', 'any single character'],
        ['\\d', 'digit'],
        ['\\w', 'word character'],
        ['\\s', 'whitespace'],
        ['[abc]', 'a, b, or c'],
        ['[^abc]', 'not a, b, or c']
      ]
    },
    {
      title: 'Repeat',
      rows: [
        ['*', 'zero or more'],
        ['+', 'one or more'],
        ['?', 'zero or one'],
        ['{3}', 'exactly 3'],
        ['{2,5}', 'between 2 and 5']
      ]
    },
    {
      title: 'Position',
      rows: [
        ['^', 'start of line'],
        ['$', 'end of line'],
        ['\\b', 'word boundary']
      ]
    },
    {
      title: 'Groups',
      rows: [
        ['cat|dog', 'cat or dog'],
        ['(foo)', 'capture group'],
        ['(?:foo)', 'non-capturing group']
      ]
    }
  ];

  const examples = [
    ['spin_\\w+', 'spin_ followed by word characters'],
    ['^\\s*#include', 'include lines with optional leading spaces'],
    ['\\berror\\b', 'the word error, not terror'],
    ['foo.*bar', 'foo then bar on the same line']
  ];

  function closeFromBackdrop(event: MouseEvent) {
    if (event.target === event.currentTarget) {
      onClose?.();
    }
  }
</script>

<div class="modal-layer" role="presentation" onclick={closeFromBackdrop}>
  <div class="cheat-sheet-dialog" role="dialog" aria-modal="true" aria-labelledby="regex-cheat-sheet-title">
    <header>
      <h2 id="regex-cheat-sheet-title">Regex Cheat Sheet</h2>
      <button type="button" aria-label="Close regex cheat sheet" onclick={onClose}>Close</button>
    </header>

    <div class="content">
      {#each sections as section}
        <section aria-labelledby={`regex-section-${section.title}`}>
          <h3 id={`regex-section-${section.title}`}>{section.title}</h3>
          <div class="rows">
            {#each section.rows as row}
              <div class="row">
                <code>{row[0]}</code>
                <span>{row[1]}</span>
              </div>
            {/each}
          </div>
        </section>
      {/each}

      <section class="examples" aria-labelledby="regex-examples">
        <h3 id="regex-examples">Examples</h3>
        <div class="rows">
          {#each examples as example}
            <div class="row">
              <code>{example[0]}</code>
              <span>{example[1]}</span>
            </div>
          {/each}
        </div>
      </section>

      <p class="note">
        Searchmonkey uses ripgrep-style regex. Look-around and backreferences are not supported.
      </p>
    </div>
  </div>
</div>

<style>
  .modal-layer {
    position: fixed;
    top: 0;
    right: 0;
    bottom: 0;
    left: 0;
    z-index: 46;
    display: grid;
    box-sizing: border-box;
    place-items: center;
    padding: 20px;
    background: rgba(30, 37, 45, 0.22);
    overflow: auto;
  }

  .cheat-sheet-dialog {
    position: relative;
    z-index: 1;
    display: grid;
    width: min(720px, 100%);
    max-width: 100%;
    max-height: min(760px, 100%);
    grid-template-rows: auto minmax(0, 1fr);
    border: 1px solid #dfe5e0;
    border-radius: 12px;
    background: #ffffff;
    box-shadow: 0 18px 38px rgba(30, 37, 45, 0.16);
    overflow: hidden;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    border-bottom: 1px solid #e5e9ed;
    padding: 14px 16px;
  }

  h2,
  h3 {
    margin: 0;
    color: #1c232b;
  }

  h2 {
    font-size: 18px;
    font-weight: 780;
  }

  h3 {
    font-size: 13px;
    font-weight: 780;
  }

  button {
    height: 32px;
    border: 1px solid #cbd4dc;
    border-radius: 8px;
    padding: 0 12px;
    color: #1c232b;
    background: #f8fafb;
    font: inherit;
    font-size: 13px;
    font-weight: 700;
    cursor: pointer;
  }

  button:hover,
  button:focus-visible {
    border-color: #9facb8;
    outline: none;
  }

  .content {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 18px;
    padding: 16px;
    overflow: auto;
  }

  section {
    display: grid;
    gap: 8px;
    min-width: 0;
  }

  .rows {
    display: grid;
    border: 1px solid #e5e9ed;
    border-radius: 8px;
    overflow: hidden;
  }

  .row {
    display: grid;
    grid-template-columns: minmax(92px, auto) minmax(0, 1fr);
    gap: 10px;
    align-items: center;
    min-height: 34px;
    border-bottom: 1px solid #edf0f3;
    padding: 6px 10px;
  }

  .row:last-child {
    border-bottom: 0;
  }

  code {
    color: #0f5f3a;
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: 12px;
    font-weight: 750;
    white-space: nowrap;
  }

  span,
  .note {
    color: #59656f;
    font-size: 13px;
    line-height: 1.35;
  }

  .examples,
  .note {
    grid-column: 1 / -1;
  }

  .examples .row {
    grid-template-columns: minmax(160px, auto) minmax(0, 1fr);
  }

  .note {
    margin: 0;
    border: 1px solid #dce8df;
    border-radius: 8px;
    padding: 10px 12px;
    background: #f7fbf8;
    font-weight: 650;
  }

  @media (max-width: 620px) {
    .modal-layer {
      align-items: end;
      padding: 10px;
    }

    .cheat-sheet-dialog {
      max-width: 100%;
      max-height: 100%;
    }

    .content {
      grid-template-columns: minmax(0, 1fr);
      gap: 14px;
      padding: 12px;
    }

    .row,
    .examples .row {
      grid-template-columns: minmax(86px, auto) minmax(0, 1fr);
    }
  }
</style>
