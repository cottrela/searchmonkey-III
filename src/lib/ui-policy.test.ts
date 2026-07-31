import { describe, expect, it, vi } from 'vitest';
import { shouldAllowNativeContextMenu, shouldOpenPathSuggestions } from './ui-policy';

describe('path suggestion policy', () => {
  it('keeps suggestions closed after selecting a directory with Browse', () => {
    expect(shouldOpenPathSuggestions(true, true, 3)).toBe(false);
  });

  it('opens available suggestions after suppression is cleared by user input', () => {
    expect(shouldOpenPathSuggestions(false, true, 3)).toBe(true);
  });
});

describe('native context menu policy', () => {
  it('blocks the WebView context menu on unused application space', () => {
    const target = { closest: vi.fn(() => null) } as unknown as Element;

    expect(shouldAllowNativeContextMenu(target)).toBe(false);
  });

  it.each(['input', 'textarea', '[contenteditable="true"]', '.source'])(
    'preserves useful native actions within %s',
    () => {
      const target = { closest: vi.fn(() => ({})) } as unknown as Element;

      expect(shouldAllowNativeContextMenu(target)).toBe(true);
    }
  );
});
