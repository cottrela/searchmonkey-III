const PATH_SEPARATOR = /[/\\]/;
const WINDOWS_DRIVE = /^[A-Za-z]:/;

export function preferredPathSeparator(path: string) {
  if (path.includes('\\')) return '\\';
  if (WINDOWS_DRIVE.test(path)) return '\\';
  return '/';
}

export function ensureTrailingPathSeparator(path: string) {
  if (!path || PATH_SEPARATOR.test(path.at(-1) ?? '')) return path;
  return `${path}${preferredPathSeparator(path)}`;
}

export function filename(filePath: string) {
  const parts = filePath.split(PATH_SEPARATOR).filter(Boolean);
  return parts.at(-1) || filePath;
}

export function parentPath(filePath: string) {
  const slashIndex = Math.max(filePath.lastIndexOf('/'), filePath.lastIndexOf('\\'));
  if (slashIndex <= 0) return filePath;
  return filePath.slice(0, slashIndex);
}

export function normalizeGlobPattern(pattern: string) {
  return pattern.replace(/\\/g, '/');
}
