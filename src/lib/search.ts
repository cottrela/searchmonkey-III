import { Channel, invoke } from '@tauri-apps/api/core';
import type { FilePreview, SearchMatch, SearchRequest, SearchStreamEvent } from './types';

export async function searchFiles(request: SearchRequest): Promise<SearchMatch[]> {
  return invoke<SearchMatch[]>('search_files', { request });
}

export async function readFilePreview(
  path: string,
  startLine: number,
  endLine: number
): Promise<FilePreview> {
  return invoke<FilePreview>('read_file_preview', { path, startLine, endLine });
}

export async function homeDir(): Promise<string> {
  return invoke<string>('home_dir');
}

export async function listDirectory(path: string, includeHidden = false): Promise<string[]> {
  return invoke<string[]>('list_directory', { path, includeHidden });
}

export async function startSearch(
  request: SearchRequest,
  searchId: number,
  onEvent: (event: SearchStreamEvent) => void
): Promise<number> {
  const events = new Channel<SearchStreamEvent>(onEvent);

  return invoke<number>('start_search', { request, searchId, events });
}

export async function stopSearch(searchId: number): Promise<void> {
  return invoke<void>('stop_search', { searchId });
}
