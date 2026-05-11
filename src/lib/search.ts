import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type {
  FilePreview,
  PluginIndexStatus,
  SearchBufferUpdatedEvent,
  SearchMatch,
  SearchRequest,
  SearchStatus,
  SearchStatusChangedEvent
} from './types';

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

export async function openFilePath(path: string): Promise<void> {
  return invoke<void>('open_file_path', { path });
}

export async function revealFilePath(path: string): Promise<void> {
  return invoke<void>('reveal_file_path', { path });
}

export async function copyTextNative(text: string): Promise<void> {
  return invoke<void>('copy_text', { text });
}

export async function homeDir(): Promise<string> {
  return invoke<string>('home_dir');
}

export async function listDirectory(path: string, includeHidden = false): Promise<string[]> {
  return invoke<string[]>('list_directory', { path, includeHidden });
}

export async function listenSearchBufferUpdated(onEvent: (event: SearchBufferUpdatedEvent) => void): Promise<() => void> {
  return listen<SearchBufferUpdatedEvent>('search_buffer_updated', (event) => onEvent(event.payload));
}

export async function listenSearchStatusChanged(onEvent: (event: SearchStatusChangedEvent) => void): Promise<() => void> {
  return listen<SearchStatusChangedEvent>('search_status_changed', (event) => onEvent(event.payload));
}

export async function startSearch(request: SearchRequest): Promise<number> {
  return invoke<number>('start_search', { request });
}

export async function getSearchStatus(searchId: number): Promise<SearchStatus> {
  return invoke<SearchStatus>('get_search_status', { searchId });
}

export async function getResults(searchId: number, offset: number, limit: number): Promise<SearchMatch[]> {
  return invoke<SearchMatch[]>('get_results', { searchId, offset, limit });
}

export async function cancelSearch(searchId: number): Promise<void> {
  return invoke<void>('cancel_search', { searchId });
}

export async function clearSearch(searchId: number): Promise<void> {
  return invoke<void>('clear_search', { searchId });
}

export async function getPluginIndexStatus(): Promise<PluginIndexStatus> {
  return invoke<PluginIndexStatus>('get_plugin_index_status');
}

export async function setPluginIndexPaused(paused: boolean): Promise<PluginIndexStatus> {
  return invoke<PluginIndexStatus>('set_plugin_index_paused', { paused });
}

export async function rebuildPluginIndex(): Promise<PluginIndexStatus> {
  return invoke<PluginIndexStatus>('rebuild_plugin_index');
}

export async function pluginFolderPath(): Promise<string> {
  return invoke<string>('plugin_folder_path');
}

export async function queuePluginScan(path: string): Promise<PluginIndexStatus> {
  return invoke<PluginIndexStatus>('queue_plugin_scan', { path });
}

export async function ignorePluginIssue(path: string, pluginId: string): Promise<PluginIndexStatus> {
  return invoke<PluginIndexStatus>('ignore_plugin_issue', { path, pluginId });
}

export async function unignorePluginIssue(path: string, pluginId: string): Promise<PluginIndexStatus> {
  return invoke<PluginIndexStatus>('unignore_plugin_issue', { path, pluginId });
}
