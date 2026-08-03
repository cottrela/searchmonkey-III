import { describe, expect, it } from 'vitest';
import { expandCommandTemplate, normalizeExtension, openerForPath, parseCommandTemplate, type FileOpenersConfig } from './file-openers';

describe('file opener rules', () => {
  it('normalizes common extension forms', () => {
    expect(normalizeExtension('*.TXT')).toBe('txt');
    expect(normalizeExtension('.Md')).toBe('md');
  });

  it('uses the first matching extension rule', () => {
    const config: FileOpenersConfig = {
      rules: [
        { extension: 'txt', template: 'first {path}' },
        { extension: 'txt', template: 'second {path}' }
      ]
    };
    expect(openerForPath('/tmp/README.TXT', config)?.template).toBe('first {path}');
    expect(openerForPath('/tmp/README', config)).toBeNull();
  });

  it('parses a quoted binary and previews placeholders', () => {
    expect(parseCommandTemplate('"/Applications/My Editor" {path} --line {line}')).toEqual({
      command: '/Applications/My Editor',
      arguments: ['{path}', '--line', '{line}']
    });
    expect(expandCommandTemplate('editor {path}:{line}', '/example/file.txt')).toBe('editor /example/file.txt:42');
  });
});
