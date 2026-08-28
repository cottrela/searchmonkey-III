import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

describe('search bar', () => {
  it('uses the field and Search button without redundant visible instructions', () => {
    const searchBar = readFileSync(
      new URL('./components/SearchBar.svelte', import.meta.url),
      'utf8'
    );

    expect(searchBar).not.toContain('class="query-meta"');
    expect(searchBar).not.toContain('Enter Search');
    expect(searchBar).toContain('aria-label="Search text"');
    expect(searchBar).toContain('placeholder="Search text"');
  });

  it('keeps the optional file or folder name refinement compact', () => {
    const searchBar = readFileSync(
      new URL('./components/SearchBar.svelte', import.meta.url),
      'utf8'
    );

    expect(searchBar).toContain('>+ Name</button>');
    expect(searchBar).toContain('aria-label="File or folder name"');
    expect(searchBar).toContain('{#if showPathQuery}');
  });
});
