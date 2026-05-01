import { normalizeGlobPattern } from './paths';

const GLOB_CHARACTERS = /[*?[\]{}]/;
const PATH_SEPARATOR = /[/\\]/;

export function normalizeIncludePatterns(input: string[]): string[] {
  return input.map((rawPattern) => {
    const pattern = normalizeGlobPattern(rawPattern);
    if (hasPathSeparator(pattern)) return normalizePathPattern(pattern);
    if (hasGlobCharacters(pattern)) return pattern;

    return pattern.startsWith('.') ? `*${pattern}` : `*.${pattern}`;
  });
}

export function normalizeExcludePatterns(input: string[]): string[] {
  return input.map((rawPattern) => {
    const pattern = normalizeGlobPattern(rawPattern);
    if (hasPathSeparator(pattern)) return normalizePathPattern(pattern);
    if (hasGlobCharacters(pattern) || hasFileExtension(pattern)) return pattern;

    return `**/${pattern}/**`;
  });
}

function hasGlobCharacters(pattern: string): boolean {
  return GLOB_CHARACTERS.test(pattern);
}

function hasPathSeparator(pattern: string): boolean {
  return PATH_SEPARATOR.test(pattern);
}

function hasFileExtension(pattern: string): boolean {
  const lastSegment = pattern.split(PATH_SEPARATOR).pop() ?? pattern;
  const dotIndex = lastSegment.lastIndexOf('.');

  return dotIndex > 0 && dotIndex < lastSegment.length - 1;
}

function normalizePathPattern(pattern: string): string {
  if (isAnchoredPattern(pattern)) return pattern;

  return `**/${pattern}`;
}

function isAnchoredPattern(pattern: string): boolean {
  return (
    pattern.startsWith('/') ||
    pattern.startsWith('\\') ||
    pattern.startsWith('./') ||
    pattern.startsWith('.\\') ||
    pattern.startsWith('**/')
  );
}
