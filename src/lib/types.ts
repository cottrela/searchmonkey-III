export type SearchRequest = {
  query: string;
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
  encoding: 'auto' | 'utf-8' | 'ascii';
  max_matches: number;
  respect_gitignore: boolean;
  ignore_node_modules: boolean;
  ignore_build_artifacts: boolean;
};

export type SearchMatch = {
  path: string;
  line_number: number;
  line_text: string;
  submatches: SearchSubmatch[];
  file_size: number | null;
  modified_secs: number | null;
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
};

export type SearchMode = 'literal' | 'regex';
export type ModifiedPreset = 'any' | '24h' | '7d' | '30d' | 'custom';
export type FileTypeFilter = 'all' | 'text' | 'code' | 'logs' | 'custom';
export type ResultSort = 'relevance' | 'file_name' | 'modified_date' | 'match_count';
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
  path: string;
  includePatterns: string;
  excludePatterns: string;
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
    max_matches: 10000,
    respect_gitignore: true,
    ignore_node_modules: true,
    ignore_build_artifacts: true,
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

export type SearchState = 'idle' | 'searching' | 'stopping' | 'done' | 'error';

export type FileResultGroup = {
  path: string;
  matches: SearchMatch[];
};

export type PreviewState = {
  filePath: string;
  filePreview: FilePreview | null;
  matches: SearchMatch[];
  activeMatchIndex: number;
  activeMatch: SearchMatch | null;
};

export type SearchStreamEvent =
  | SearchStartedEvent
  | SearchBatchEvent
  | SearchErrorEvent
  | SearchCompleteEvent
  | {
      type: 'cancelled';
      search_id: number;
      total_matches: number;
    };

export type SearchStartedEvent = {
  type: 'started';
  search_id: number;
};

export type SearchBatchEvent = {
  type: 'batch';
  search_id: number;
  results: SearchMatch[];
};

export type SearchErrorEvent = {
  type: 'error';
  search_id: number;
  message: string;
};

export type SearchCompleteEvent = {
  type: 'finished';
  search_id: number;
  total_matches: number;
};
