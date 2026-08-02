export type SearchRequest = {
  query: string;
  path_query: string;
  path: string;
  regex: boolean;
  case_sensitive: boolean;
  hidden: boolean;
  include_patterns: string[];
  exclude_patterns: string[];
  follow_symlinks: boolean;
  multiline: boolean;
  context_lines: number;
  min_file_size: string;
  max_file_size: string;
  modified_after: number | null;
  skip_binary: boolean;
  encoding: 'auto' | 'utf-8' | 'windows-1250' | 'ascii';
  max_matches: number;
  respect_gitignore: boolean;
  ignore_node_modules: boolean;
  ignore_build_artifacts: boolean;
};

export type CliLaunchRequest = {
  query: string | null;
  path: string | null;
  regex: boolean;
  case_sensitive: boolean;
  globs: string[];
  hidden: boolean;
  follow_symlinks: boolean;
  context_lines: number | null;
  no_ignore: boolean;
  start: boolean;
};

export type SearchMatch = {
  path: string;
  preview_path?: string | null;
  display_context?: string | null;
  plugin_id?: string | null;
  meta_outdated?: boolean | null;
  line_number: number;
  line_text: string;
  submatches: SearchSubmatch[];
  absolute_offset?: number | null;
  file_size: number | null;
  modified_secs: number | null;
};

export type SearchBufferUpdatedEvent = {
  search_id: number;
  total_matches: number;
};

export type BackendSearchState = 'Starting' | 'Running' | 'Cancelling' | 'Completed' | 'Cancelled' | 'Failed';

export type SearchStatusChangedEvent = {
  search_id: number;
  state: BackendSearchState;
};

export type SearchStatus = {
  search_id: number;
  state: BackendSearchState;
  total_matches: number;
  error_message: string | null;
};

export type SearchSubmatch = {
  start: number;
  end: number;
};

export type FilePreview = {
  path: string;
  start_line: number;
  end_line: number;
  lines: FilePreviewLine[];
  truncated: boolean;
};

export type FilePreviewLine = {
  number: number;
  text: string;
  is_match: boolean;
  match_ranges: SearchSubmatch[];
  page_breaks?: FilePreviewPageBreak[];
};

export type FilePreviewPageBreak = {
  page?: number | null;
  label?: string | null;
};

export type SearchMode = 'literal' | 'regex';
export type ModifiedPreset = 'any' | '24h' | '7d' | '30d' | 'custom';
export type FileTypeFilter = 'all' | 'text' | 'code' | 'logs' | 'custom';
export type ResultSort = 'relevance' | 'file_name' | 'path' | 'modified_date' | 'match_count' | 'file_size';
export type ResultSortDirection = 'desc' | 'asc';

export type SearchOptions = Pick<
  SearchRequest,
  | 'regex'
  | 'case_sensitive'
  | 'hidden'
  | 'follow_symlinks'
  | 'multiline'
  | 'context_lines'
  | 'min_file_size'
  | 'max_file_size'
  | 'modified_after'
  | 'skip_binary'
  | 'encoding'
  | 'max_matches'
  | 'respect_gitignore'
  | 'ignore_node_modules'
  | 'ignore_build_artifacts'
> & {
  search_mode: SearchMode;
  modified_preset: ModifiedPreset;
  modified_custom_days: number;
  file_type: FileTypeFilter;
  custom_file_type: string;
  sort_by: ResultSort;
  sort_direction: ResultSortDirection;
  show_line_numbers: boolean;
  group_by_file: boolean;
};

export type SearchCriteria = {
  id: string;
  name: string;
  query: string;
  pathQuery: string;
  path: string;
  includePatterns: string[];
  excludePatterns: string[];
  options: SearchOptions;
};

export function defaultSearchOptions(): SearchOptions {
  return {
    regex: false,
    case_sensitive: false,
    hidden: false,
    follow_symlinks: false,
    multiline: false,
    context_lines: 0,
    min_file_size: '',
    max_file_size: '10M',
    modified_after: null,
    skip_binary: true,
    encoding: 'auto',
    max_matches: 100000,
    respect_gitignore: true,
    ignore_node_modules: false,
    ignore_build_artifacts: false,
    search_mode: 'literal',
    modified_preset: 'any',
    modified_custom_days: 14,
    file_type: 'all',
    custom_file_type: '',
    sort_by: 'relevance',
    sort_direction: 'desc',
    show_line_numbers: true,
    group_by_file: true
  };
}

export type SearchState = 'idle' | 'starting' | 'running' | 'cancelling' | 'completed' | 'cancelled' | 'failed';

export type FileResultGroup = {
  path: string;
  matches: SearchMatch[];
};

export type PreviewState = {
  filePath: string;
  thumbnailPath: string;
  filePreview: FilePreview | null;
  matches: SearchMatch[];
  activeMatchIndex: number;
  activeMatch: SearchMatch | null;
};

export type PluginCapabilitySummary = {
  text: boolean;
  layout: boolean;
  ocr: boolean;
};

export type InstalledPluginInfo = {
  id: string;
  name: string;
  version: string;
  is_active: boolean;
  enabled: boolean;
  requires_entitlement: boolean;
  handles: string[];
  root_path: string;
  capabilities: PluginCapabilitySummary;
};

export type PluginHealthSummary = {
  plugin_id: string;
  indexed_count: number;
  attention_count: number;
  ignored_count: number;
  queued_count: number;
  processing_count: number;
  blocked_count: number;
};

export type PluginIssue = {
  source_path: string;
  file_name: string;
  plugin_id: string;
  status: string;
  error_code: string;
  message: string;
  details: string;
  attempts: number;
  retry_after?: string | null;
  last_reported_at: string;
};

export type PluginIssuePreference = {
  plugin_id: string;
  error_code: string;
};

export type PluginValidationError = {
  plugin_id: string;
  plugin_name: string;
  version: string;
  message: string;
};

export type PurchaseConnectionState = 'not_connected' | 'pending' | 'connected' | 'expired';

export type PurchaseConnectionSummary = {
  state: PurchaseConnectionState;
  email: string | null;
  pending_email: string | null;
  pending_expires_at: string | null;
  last_synced_at: string | null;
  has_cached_entitlements: boolean;
  status_message: string | null;
  storage_warning: string | null;
};

export type MarketplacePluginSummary = {
  plugin_id: string;
  name: string;
  owned: boolean;
  latest_version: string | null;
  download_url: string | null;
  buy_url: string | null;
  homepage_url: string | null;
};

export type PluginIssueCount = {
  plugin_id: string;
  status: string;
  error_code: string;
  count: number;
};

export type PluginIndexFailure = {
  source_path: string;
  plugin_id: string;
  attempts: number;
  code: string;
  message: string;
  details: string;
  next_retry_at?: string | null;
};

export type PluginIndexSummary = {
  enabled_plugins: string[];
  installed_plugins: InstalledPluginInfo[];
  indexing_state: string;
  plugin_state: string;
  paused: boolean;
  search_active: boolean;
  scanner_running: boolean;
  worker_running: boolean;
  plugin_summaries: PluginHealthSummary[];
  auto_ignored_issue_types: PluginIssuePreference[];
  plugin_validation_errors: PluginValidationError[];
  purchase_connection: PurchaseConnectionSummary;
  marketplace_plugins: MarketplacePluginSummary[];
};

export type InstallPluginResult = {
  plugin_id: string;
  version: string;
  status: PluginIndexSummary;
};
