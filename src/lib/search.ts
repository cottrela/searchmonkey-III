import { Channel, invoke } from '@tauri-apps/api/core';
import type { SearchMatch, SearchRequest, SearchStreamEvent } from './types';

export async function searchFiles(request: SearchRequest): Promise<SearchMatch[]> {
  return invoke<SearchMatch[]>('search_files', { request });
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
