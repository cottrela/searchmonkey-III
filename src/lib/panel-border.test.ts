import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

describe('results/preview boundary', () => {
  it('has one border owner instead of stacking a preview inset border on the splitter', () => {
    const previewPanel = readFileSync(
      new URL('./components/PreviewPanel.svelte', import.meta.url),
      'utf8'
    );
    const page = readFileSync(new URL('../routes/+page.svelte', import.meta.url), 'utf8');

    expect(previewPanel).not.toMatch(/\.preview-panel\s*\{[^}]*box-shadow:\s*inset 1px 0 0/s);
    expect(page).toMatch(/\.panel-resizer\s*\{[^}]*border-right:\s*1px solid var\(--border\)/s);
  });
});
