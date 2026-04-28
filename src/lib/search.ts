import { invoke } from '@tauri-apps/api/core';
import type { SearchMatch, SearchRequest } from './types';

export async function searchFiles(request: SearchRequest): Promise<SearchMatch[]> {
  return invoke<SearchMatch[]>('search_files', { request });
}
