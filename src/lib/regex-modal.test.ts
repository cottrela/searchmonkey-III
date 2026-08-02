import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

describe('regex cheat sheet modal', () => {
  const source = readFileSync(
    new URL('./components/RegexCheatSheetDialog.svelte', import.meta.url),
    'utf8'
  );

  it('owns a viewport-anchored backdrop without relying on viewport units', () => {
    expect(source).toMatch(/\.modal-layer\s*\{[^}]*position:\s*fixed/s);
    expect(source).toMatch(/\.modal-layer\s*\{[^}]*top:\s*0[^}]*right:\s*0[^}]*bottom:\s*0[^}]*left:\s*0/s);
    expect(source).toMatch(/\.modal-layer\s*\{[^}]*box-sizing:\s*border-box/s);
    expect(source).toMatch(/\.modal-layer\s*\{[^}]*background:\s*rgba\(30, 37, 45, 0\.22\)/s);
    expect(source).not.toMatch(/\b(?:100vw|100vh)\b/);
  });

  it('only treats clicks on the backdrop itself as close requests', () => {
    expect(source).toContain('event.target === event.currentTarget');
    expect(source).toContain('onclick={closeFromBackdrop}');
    expect(source).not.toContain('class="modal-backdrop"');
  });
});
