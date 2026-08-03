export type FileOpenerRule = {
  extension: string;
  template: string;
};

export type FileOpenersConfig = {
  rules: FileOpenerRule[];
};

const STORAGE_KEY = 'searchmonkey.file-openers.v1';

export function defaultFileOpenersConfig(): FileOpenersConfig {
  return { rules: [] };
}

export function normalizeExtension(value: string) {
  return value.trim().replace(/^\*?\./, '').toLowerCase();
}

export function loadFileOpenersConfig(): FileOpenersConfig {
  try {
    const parsed = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '{}');
    if (!Array.isArray(parsed.rules)) return defaultFileOpenersConfig();

    const rules: FileOpenerRule[] = [];
    for (const value of parsed.rules.filter((rule: unknown) => rule && typeof rule === 'object')) {
      if (typeof value.extension === 'string' && typeof value.template === 'string') {
        const extension = normalizeExtension(value.extension);
        if (extension && value.template.trim()) rules.push({ extension, template: value.template.trim() });
        continue;
      }

      // Migrate the original multi-extension command/argument format.
      if (Array.isArray(value.extensions) && typeof value.command === 'string') {
        const template = [quoteTemplateToken(value.command), ...(Array.isArray(value.arguments) ? value.arguments : [])].join(' ').trim();
        for (const item of value.extensions) {
          const extension = normalizeExtension(String(item));
          if (extension && template) rules.push({ extension, template });
        }
      }
    }
    return { rules };
  } catch {
    return defaultFileOpenersConfig();
  }
}

export function saveFileOpenersConfig(config: FileOpenersConfig) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(config));
}

export function openerForPath(path: string, config: FileOpenersConfig): FileOpenerRule | null {
  const filename = path.split(/[\\/]/).at(-1) ?? '';
  const extension = normalizeExtension(filename.includes('.') ? filename.split('.').at(-1) ?? '' : '');
  if (!extension) return null;
  return config.rules.find((rule) => rule.extension === extension) ?? null;
}

export function parseCommandTemplate(template: string): { command: string; arguments: string[] } | null {
  const tokens: string[] = [];
  let token = '';
  let quote = '';
  let started = false;

  for (const character of template.trim()) {
    if (quote) {
      if (character === quote) quote = '';
      else token += character;
      started = true;
    } else if (character === '"' || character === "'") {
      quote = character;
      started = true;
    } else if (/\s/.test(character)) {
      if (started) {
        tokens.push(token);
        token = '';
        started = false;
      }
    } else {
      token += character;
      started = true;
    }
  }
  if (started) tokens.push(token);
  if (!tokens[0]) return null;
  return { command: tokens[0], arguments: tokens.slice(1) };
}

export function expandCommandTemplate(template: string, path: string, line = 42, column = 7) {
  return template
    .replaceAll('{path}', path)
    .replaceAll('{line}', String(line))
    .replaceAll('{column}', String(column));
}

export function binaryFromTemplate(template: string) {
  return parseCommandTemplate(template)?.command ?? '';
}

export function quoteTemplateToken(value: string) {
  const trimmed = value.trim();
  return /\s/.test(trimmed) ? `"${trimmed.replaceAll('"', '\\"')}"` : trimmed;
}
