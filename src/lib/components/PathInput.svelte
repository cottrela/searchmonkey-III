<script lang="ts">
  import { tick } from 'svelte';
  import { open } from '@tauri-apps/plugin-dialog';
  import { listDirectory } from '$lib/search';
  import { ensureTrailingPathSeparator, preferredPathSeparator } from '$lib/paths';
  import { shouldOpenPathSuggestions } from '$lib/ui-policy';

  let {
    id,
    value = $bindable(''),
    placeholder = '',
    includeHidden = false
  }: {
    id: string;
    value: string;
    placeholder?: string;
    includeHidden?: boolean;
  } = $props();

  let inputElement = $state<HTMLInputElement>();
  let suggestionsElement = $state<HTMLDivElement>();
  let suggestions = $state<string[]>([]);
  let activeIndex = $state(0);
  let openSuggestions = $state(false);
  let cursorPosition = $state(0);
  let loading = $state(false);
  let error = $state('');
  let requestId = 0;
  let debounceTimer: ReturnType<typeof setTimeout> | undefined;
  let suggestionContext: PathContext | null = null;
  let breadcrumbsExpanded = $state(false);
  let suggestionsSuppressed = false;

  const MAX_VISIBLE_SUGGESTIONS = 80;
  const separatorPattern = /[\\/]/;
  const windowsDriveRootPattern = /^[A-Za-z]:[\\/]?$/;

  type PathContext = {
    basePath: string;
    query: string;
    segmentStart: number;
    segmentEnd: number;
    appendToDirectory?: boolean;
  };

  type PathSegment = {
    label: string;
    path: string;
    ellipsis?: boolean;
  };

  const visibleSuggestions = $derived(suggestions.slice(0, MAX_VISIBLE_SUGGESTIONS));
  const pathSegments = $derived.by(() => buildPathSegments(value));
  const visiblePathSegments = $derived.by(() =>
    breadcrumbsExpanded ? pathSegments : compressPathSegments(pathSegments)
  );

  $effect(() => {
    const nextValue = value;
    const nextCursor = cursorPosition;
    const nextIncludeHidden = includeHidden;

    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      void refreshSuggestions(nextValue, nextCursor, nextIncludeHidden);
    }, 90);

    return () => clearTimeout(debounceTimer);
  });

  function pathContext(pathValue: string, cursor: number): PathContext {
    const boundedCursor = Math.max(0, Math.min(cursor, pathValue.length));
    const beforeCursor = pathValue.slice(0, boundedCursor);
    const lastSlash = Math.max(beforeCursor.lastIndexOf('/'), beforeCursor.lastIndexOf('\\'));
    const nextSlashOffset = pathValue.slice(boundedCursor).search(separatorPattern);
    const drivePrefixLength = /^[A-Za-z]:/.test(pathValue) ? 2 : 0;
    const segmentStart = Math.max(lastSlash + 1, drivePrefixLength);
    const segmentEnd = nextSlashOffset === -1 ? pathValue.length : boundedCursor + nextSlashOffset;
    const basePath = segmentStart === 0 ? '.' : pathValue.slice(0, segmentStart);
    const query = pathValue.slice(segmentStart, boundedCursor);

    return { basePath, query, segmentStart, segmentEnd };
  }

  async function refreshSuggestions(pathValue: string, cursor: number, showHidden: boolean) {
    const initialContext = pathContext(pathValue, cursor);
    const currentRequest = ++requestId;

    loading = true;
    error = '';

    try {
      if (isCursorAtEndOfDirectory(pathValue, cursor)) {
        try {
          const entries = await listDirectory(pathValue, showHidden);
          if (currentRequest !== requestId) return;

          suggestionContext = {
            basePath: pathValue,
            query: '',
            segmentStart: pathValue.length,
            segmentEnd: pathValue.length,
            appendToDirectory: true
          };
          suggestions = entries;
          activeIndex = 0;
          openSuggestions = shouldOpenPathSuggestions(
            suggestionsSuppressed,
            document.activeElement === inputElement,
            suggestions.length
          );
          return;
        } catch {
          // Fall back to sibling completion for partial directory names.
        }
      }

      const context = initialContext;
      const entries = await listDirectory(context.basePath, showHidden);
      if (currentRequest !== requestId) return;

      const query = context.query.toLocaleLowerCase();
      suggestionContext = context;
      suggestions = entries.filter((entry) => entry.toLocaleLowerCase().startsWith(query));
      activeIndex = 0;
      openSuggestions = shouldOpenPathSuggestions(
        suggestionsSuppressed,
        document.activeElement === inputElement,
        suggestions.length
      );
    } catch {
      if (currentRequest !== requestId) return;
      suggestionContext = null;
      suggestions = [];
      openSuggestions = false;
      error = initialContext.basePath === '.' && !pathValue ? '' : 'No suggestions';
    } finally {
      if (currentRequest === requestId) {
        loading = false;
      }
    }
  }

  function updateCursor() {
    cursorPosition = inputElement?.selectionStart ?? value.length;
  }

  async function acceptSuggestion(index = activeIndex) {
    const suggestion = visibleSuggestions[index];
    if (!suggestion) return;

    const context = suggestionContext ?? pathContext(value, cursorPosition);
    const prefix = context.appendToDirectory
      ? ensureTrailingSeparator(value.slice(0, context.segmentStart))
      : value.slice(0, context.segmentStart);
    const suffix = context.appendToDirectory ? '' : value.slice(context.segmentEnd);
    const insertedSuggestion = stripDirectoryMarker(suggestion);
    const fallbackValue = `${prefix}${insertedSuggestion}`;
    const candidateValue = joinPathEdit(prefix, insertedSuggestion, suffix);
    const nextValue = await validDirectory(candidateValue) ? candidateValue : fallbackValue;

    value = nextValue;
    cursorPosition = suffix && nextValue === candidateValue ? fallbackValue.length : nextValue.length;
    openSuggestions = false;

    void tick().then(() => {
      inputElement?.focus();
      inputElement?.setSelectionRange(cursorPosition, cursorPosition);
    });
  }

  async function validDirectory(path: string) {
    try {
      await listDirectory(path, includeHidden);
      return true;
    } catch {
      return false;
    }
  }

  function joinPathEdit(prefix: string, suggestion: string, suffix: string) {
    if (!suffix) return `${prefix}${suggestion}`;

    const needsSeparator =
      !suggestion.endsWith('/') &&
      !suggestion.endsWith('\\') &&
      !suffix.startsWith('/') &&
      !suffix.startsWith('\\');
    const nextSuffix = needsSeparator ? `${preferredPathSeparator(prefix || value)}${suffix}` : suffix;

    return `${prefix}${suggestion}${nextSuffix}`;
  }

  function stripDirectoryMarker(suggestion: string) {
    return suggestion.replace(/[\\/]+$/, '');
  }

  function isCursorAtEndOfDirectory(pathValue: string, cursor: number) {
    return (
      pathValue.length > 0 &&
      cursor === pathValue.length &&
      (!/[\\/]$/.test(pathValue) || windowsDriveRootPattern.test(pathValue))
    );
  }

  function ensureTrailingSeparator(pathValue: string) {
    return ensureTrailingPathSeparator(pathValue);
  }

  async function browseForDirectory() {
    const selected = await open({
      directory: true,
      multiple: false,
      defaultPath: value || undefined
    });

    if (typeof selected !== 'string') return;

    value = selected;
    cursorPosition = selected.length;
    suggestionsSuppressed = true;
    openSuggestions = false;

    await tick();
    inputElement?.focus();
    inputElement?.setSelectionRange(cursorPosition, cursorPosition);
  }

  function handleInput() {
    suggestionsSuppressed = false;
    updateCursor();
    openSuggestions = true;
  }

  function handleFocus() {
    updateCursor();
    openSuggestions = shouldOpenPathSuggestions(
      suggestionsSuppressed,
      true,
      suggestions.length
    );
  }

  function handleBlur() {
    openSuggestions = false;
  }

  function handleKeydown(event: KeyboardEvent) {
    updateCursor();

    if (event.key === 'ArrowDown') {
      if (!visibleSuggestions.length) return;
      event.preventDefault();
      openSuggestions = true;
      activeIndex = Math.min(activeIndex + 1, visibleSuggestions.length - 1);
      scrollActiveSuggestionIntoView();
      return;
    }

    if (event.key === 'ArrowUp') {
      if (!visibleSuggestions.length) return;
      event.preventDefault();
      openSuggestions = true;
      activeIndex = Math.max(activeIndex - 1, 0);
      scrollActiveSuggestionIntoView();
      return;
    }

    if (event.key === 'Tab') {
      openSuggestions = false;
      return;
    }

    if (event.key === 'Enter') {
      if (!openSuggestions || !visibleSuggestions.length) return;
      event.preventDefault();
      acceptSuggestion();
      return;
    }

    if (event.key === 'Escape') {
      openSuggestions = false;
      return;
    }

    if (event.key === 'Backspace') {
      maybeMoveToParent(event);
    }
  }

  function maybeMoveToParent(event: KeyboardEvent) {
    const selectionStart = inputElement?.selectionStart ?? 0;
    const selectionEnd = inputElement?.selectionEnd ?? 0;

    if (selectionStart !== selectionEnd || selectionStart !== value.length || value.length <= 1) {
      return;
    }

    const trimmed = value.replace(/[\\/]+$/, '');
    if (trimmed === value) return;

    const lastSlash = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
    if (lastSlash < 0) return;

    event.preventDefault();
    value = trimmed.slice(0, lastSlash + 1);
    cursorPosition = value.length;

    void tick().then(() => {
      inputElement?.setSelectionRange(cursorPosition, cursorPosition);
    });
  }

  function jumpToSegment(path: string) {
    value = path;
    cursorPosition = value.length;
    openSuggestions = true;

    void tick().then(() => {
      inputElement?.focus();
      inputElement?.setSelectionRange(cursorPosition, cursorPosition);
    });
  }

  function scrollActiveSuggestionIntoView() {
    void tick().then(() => {
      const activeOption = suggestionsElement?.querySelector('[aria-selected="true"]');
      activeOption?.scrollIntoView({ block: 'nearest' });
    });
  }

  function buildPathSegments(pathValue: string): PathSegment[] {
    const separator = preferredPathSeparator(pathValue);
    const driveRoot = pathValue.match(/^([A-Za-z]:)([\\/])?/);
    if (driveRoot) {
      const root = `${driveRoot[1]}${separator}`;
      const rest = pathValue.slice(driveRoot[0].length);
      const rawSegments = rest.split(separatorPattern).filter(Boolean);
      let current = root;
      const segments: PathSegment[] = [{ label: root, path: root }];

      for (const segment of rawSegments) {
        current = ensureTrailingPathSeparator(current);
        current = `${current}${segment}${separator}`;
        segments.push({
          label: segment,
          path: current.replace(/[\\/]+$/, '')
        });
      }

      return segments;
    }

    const rawSegments = pathValue.split(separatorPattern).filter(Boolean);
    let current = pathValue.startsWith('/') || pathValue.startsWith('\\') ? separator : '';
    const segments: PathSegment[] =
      pathValue.startsWith('/') || pathValue.startsWith('\\')
        ? [{ label: separator, path: separator }]
        : [];

    for (const segment of rawSegments) {
      current = current ? ensureTrailingPathSeparator(current) : current;
      current = `${current}${segment}${separator}`;
      segments.push({
        label: segment,
        path: current.replace(/[\\/]+$/, '')
      });
    }

    return segments;
  }

  function compressPathSegments(segments: PathSegment[]): PathSegment[] {
    const startsAtRoot = ['/', '\\'].includes(segments[0]?.label);
    const leadingCount = startsAtRoot ? 2 : 1;
    const tailCount = 2;

    if (segments.length <= leadingCount + tailCount) return segments;

    return [
      ...segments.slice(0, leadingCount),
      { label: '...', path: '', ellipsis: true },
      ...segments.slice(-tailCount)
    ];
  }

  function showSeparator(segments: PathSegment[], index: number) {
    return index > 0 && !['/', '\\'].includes(segments[index - 1]?.label);
  }
</script>

<div class="path-control">
  {#if visiblePathSegments.length}
    <div class="breadcrumbs" aria-label="Path segments" title={value}>
      {#each visiblePathSegments as segment, index (`${segment.path}-${index}`)}
        {#if showSeparator(visiblePathSegments, index)}
          <span aria-hidden="true">{preferredPathSeparator(value)}</span>
        {/if}

        {#if segment.ellipsis}
          <button
            class="breadcrumb-ellipsis"
            type="button"
            aria-label="Show full path"
            onclick={() => (breadcrumbsExpanded = true)}
          >
            {segment.label}
          </button>
        {:else}
          <button type="button" onclick={() => jumpToSegment(segment.path)}>{segment.label}</button>
        {/if}
      {/each}
    </div>
  {/if}

  <div class="path-entry">
    <div class="input-wrap">
      <input
        bind:this={inputElement}
        {id}
        bind:value
        {placeholder}
        spellcheck="false"
        autocomplete="off"
        aria-autocomplete="list"
        aria-expanded={openSuggestions}
        aria-controls={`${id}-suggestions`}
        oninput={handleInput}
        onfocus={handleFocus}
        onblur={handleBlur}
        onkeyup={updateCursor}
        onclick={updateCursor}
        onkeydown={handleKeydown}
      />
      <span class="chevron" aria-hidden="true">⌄</span>
      {#if openSuggestions}
        <div
          bind:this={suggestionsElement}
          id={`${id}-suggestions`}
          class="suggestions"
          role="listbox"
        >
          {#each visibleSuggestions as suggestion, index (suggestion)}
            <button
              type="button"
              role="option"
              aria-selected={index === activeIndex}
              class:active={index === activeIndex}
              onmousedown={(event) => event.preventDefault()}
              onclick={() => acceptSuggestion(index)}
            >
              {suggestion}
            </button>
          {/each}
        </div>
      {/if}
    </div>

    <button class="browse" type="button" onclick={browseForDirectory}>Browse...</button>
  </div>

  {#if error && !loading}
    <div class="path-hint">{error}</div>
  {/if}
</div>

<style>
  .path-control {
    display: grid;
    gap: 5px;
    min-width: 0;
  }

  .path-entry {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 6px;
    align-items: start;
  }

  .input-wrap {
    position: relative;
    min-width: 0;
  }

  input {
    width: 100%;
    min-width: 0;
    box-sizing: border-box;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    color: var(--text);
    background: var(--input);
    font: inherit;
    font-size: 12px;
    height: 32px;
    padding: 0 24px 0 9px;
  }

  input:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--focus);
    outline: none;
  }

  .chevron {
    position: absolute;
    top: 6px;
    right: 8px;
    color: var(--muted);
    pointer-events: none;
  }

  .browse {
    height: 32px;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    padding: 0 10px;
    color: var(--text);
    background: var(--input);
    font: inherit;
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
    white-space: nowrap;
  }

  .browse:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--focus);
    outline: none;
  }

  .suggestions {
    position: absolute;
    z-index: 20;
    top: calc(100% + 4px);
    left: 0;
    right: 0;
    max-height: 240px;
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--panel);
    box-shadow: 0 10px 28px rgba(30, 37, 45, 0.14);
    padding: 4px;
  }

  .suggestions button {
    display: block;
    width: 100%;
    height: 28px;
    border: 0;
    border-radius: 4px;
    padding: 0 8px;
    color: var(--text);
    background: transparent;
    font: inherit;
    font-size: 12px;
    text-align: left;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .suggestions button.active,
  .suggestions button:hover {
    background: var(--selection);
  }

  .breadcrumbs {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 4px;
    color: var(--muted);
    font-size: 11px;
    overflow: hidden;
    white-space: nowrap;
  }

  .breadcrumbs button {
    min-width: 0;
    max-width: 88px;
    border: 0;
    padding: 0;
    color: var(--accent);
    background: transparent;
    font: inherit;
    font-weight: 650;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .breadcrumbs button:first-of-type {
    flex: 0 0 auto;
    max-width: 24px;
  }

  .breadcrumb-ellipsis {
    flex: 0 0 auto;
    color: var(--muted);
  }

  .path-hint {
    color: var(--muted);
    font-size: 11px;
  }
</style>
