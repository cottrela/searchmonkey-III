export type SearchRequest = {
  query: string;
  path: string;
  regex: boolean;
  case_sensitive: boolean;
  hidden: boolean;
};

export type SearchMatch = {
  path: string;
  line_number: number;
  line_text: string;
};

export type SearchOptions = Pick<SearchRequest, 'regex' | 'case_sensitive' | 'hidden'>;

export type SearchState = 'idle' | 'searching' | 'stopping' | 'done' | 'error';

export type FileResultGroup = {
  path: string;
  matches: SearchMatch[];
};

export type PreviewState = {
  filePath: string;
  content: string;
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
