export type SearchRequest = {
  query: string;
  path: string;
  regex: boolean;
  case_sensitive: boolean;
  hidden: boolean;
  include_patterns: string[];
  exclude_patterns: string[];
};

export type SearchMatch = {
  path: string;
  line_number: number;
  line_text: string;
  submatches: SearchSubmatch[];
};

export type SearchSubmatch = {
  start: number;
  end: number;
};

export type FilePreview = {
  path: string;
  start_line: number;
  lines: FilePreviewLine[];
  truncated: boolean;
};

export type FilePreviewLine = {
  number: number;
  text: string;
  is_match: boolean;
  match_ranges: SearchSubmatch[];
};

export type SearchOptions = Pick<SearchRequest, 'regex' | 'case_sensitive' | 'hidden'>;

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
