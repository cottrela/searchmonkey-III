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

export type SearchState = 'idle' | 'searching' | 'done' | 'error';

export type FileResultGroup = {
  path: string;
  matches: SearchMatch[];
};
