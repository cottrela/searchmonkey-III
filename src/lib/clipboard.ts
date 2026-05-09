import { copyTextNative } from '$lib/search';

export async function copyText(text: string): Promise<boolean> {
  if (!text) return false;

  try {
    await copyTextNative(text);
    return true;
  } catch {
    return copyTextWithBrowserFallback(text);
  }
}

async function copyTextWithBrowserFallback(text: string): Promise<boolean> {
  try {
    if (!navigator.clipboard) return copyTextWithSelectionFallback(text);

    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return copyTextWithSelectionFallback(text);
  }
}

function copyTextWithSelectionFallback(text: string): boolean {
  const textArea = document.createElement('textarea');
  textArea.value = text;
  textArea.setAttribute('readonly', '');
  textArea.style.position = 'fixed';
  textArea.style.top = '0';
  textArea.style.left = '-9999px';

  document.body.append(textArea);
  textArea.select();

  try {
    return document.execCommand('copy');
  } catch {
    return false;
  } finally {
    textArea.remove();
  }
}
