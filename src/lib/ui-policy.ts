export function shouldOpenPathSuggestions(
  suppressed: boolean,
  inputIsActive: boolean,
  suggestionCount: number
) {
  return !suppressed && inputIsActive && suggestionCount > 0;
}

export function shouldAllowNativeContextMenu(target: EventTarget | null) {
  if (!target || typeof (target as Element).closest !== 'function') return false;

  return Boolean(
    (target as Element).closest('input, textarea, [contenteditable="true"], .source')
  );
}
