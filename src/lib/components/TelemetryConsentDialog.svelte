<script lang="ts">
  import { onMount } from 'svelte';
  import {
    previewTelemetryPayload,
    saveTelemetryConsent,
    type TelemetryConsent,
    type TelemetryPayload,
    type TelemetryState
  } from '$lib/telemetry';

  let {
    firstRun = false,
    telemetry,
    onClose,
    onSaved
  }: {
    firstRun?: boolean;
    telemetry: TelemetryState;
    onClose?: () => void;
    onSaved?: (state: TelemetryState) => void;
  } = $props();

  let selectedConsent: TelemetryConsent = $state('yes');
  let payloadPreview: TelemetryPayload | null = $state(null);
  let saving = $state(false);

  onMount(() => {
    selectedConsent = telemetry.consent ?? 'yes';
    void refreshPayloadPreview();
  });

  async function chooseConsent(consent: TelemetryConsent) {
    selectedConsent = consent;
    await refreshPayloadPreview();
  }

  async function refreshPayloadPreview() {
    payloadPreview = await previewTelemetryPayload(selectedConsent);
  }

  async function saveConsent() {
    saving = true;

    const nextState = await saveTelemetryConsent(selectedConsent);
    saving = false;
    onSaved?.(nextState);
    onClose?.();
  }

  async function submitConsent(consent: TelemetryConsent) {
    await chooseConsent(consent);
    await saveConsent();
  }
</script>

<div class="modal-layer" role="presentation">
  {#if !firstRun}
    <button class="modal-backdrop" type="button" aria-label="Close telemetry preferences" onclick={onClose}></button>
  {:else}
    <div class="modal-backdrop locked" aria-hidden="true"></div>
  {/if}

  <div class="consent-dialog" role="dialog" aria-modal="true" aria-labelledby="telemetry-title">
    <header>
      <div>
        <h2 id="telemetry-title">Help improve Searchmonkey?</h2>
      </div>
      {#if !firstRun}
        <button class="quiet" type="button" onclick={onClose}>Close</button>
      {/if}
    </header>

    <div class="content">
      <p>Searchmonkey may send:</p>
      <ul>
        <li>anonymous crash reports</li>
        <li>app version + OS version</li>
        <li>optional feedback you choose to provide</li>
      </ul>
      <p>Search queries, filenames, paths, and file contents are never sent.</p>

      <details class="payload-view">
        <summary>What would be sent?</summary>
        <pre>{JSON.stringify(payloadPreview, null, 2)}</pre>
      </details>

      {#if !firstRun && telemetry.lastSubmittedAt}
        <p class="status-text">
          Current preference: {telemetry.lastSubmittedConsent ?? telemetry.consent ?? 'not set'}.
          Last updated {new Date(telemetry.lastSubmittedAt).toLocaleString()}.
        </p>
      {/if}

      {#if telemetry.lastError}
        <p class="error-text">{telemetry.lastError}</p>
      {/if}
    </div>

    <footer>
      <button class="secondary-action" type="button" disabled={saving} onclick={() => submitConsent('no')}>
        {saving && selectedConsent === 'no' ? 'Saving...' : 'Decline'}
      </button>
      <button class="primary" type="button" disabled={saving} onclick={() => submitConsent('yes')}>
        {saving && selectedConsent === 'yes' ? 'Saving...' : 'Accept'}
      </button>
    </footer>
  </div>
</div>

<style>
  .modal-layer {
    position: fixed;
    inset: 0;
    z-index: 45;
    display: grid;
    align-items: center;
    justify-items: center;
    padding: 18px;
    overflow: auto;
  }

  .modal-backdrop {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    border: 0;
    padding: 0;
    background: rgba(30, 37, 45, 0.32);
  }

  .modal-backdrop.locked {
    pointer-events: none;
  }

  .consent-dialog {
    position: relative;
    z-index: 1;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    width: min(520px, 100%);
    max-width: calc(100vw - 24px);
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
    color: var(--text);
    background: var(--panel);
    box-shadow: 0 18px 42px rgba(30, 37, 45, 0.24);
  }

  header,
  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    border-bottom: 1px solid var(--border);
    padding: 12px;
    background: var(--surface);
  }

  footer {
    justify-content: flex-end;
    border-top: 1px solid var(--border);
    border-bottom: 0;
  }

  h2 {
    margin: 0;
    font-size: 15px;
  }

  .content {
    display: grid;
    gap: 12px;
    padding: 14px 12px;
  }

  p {
    margin: 0;
    color: var(--muted);
    font-size: 13px;
    line-height: 1.45;
  }

  ul {
    margin: -2px 0 0;
    padding-left: 18px;
    color: var(--muted);
    font-size: 13px;
    line-height: 1.45;
  }

  li + li {
    margin-top: 2px;
  }

  button {
    height: 34px;
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0 11px;
    color: var(--text);
    background: var(--input);
    font: inherit;
    font-size: 12px;
    font-weight: 750;
  }

  button:not(:disabled) {
    cursor: pointer;
  }

  button:hover,
  button:focus-visible {
    border-color: var(--accent-soft);
    outline: none;
  }

  .quiet {
    border-color: transparent;
    color: var(--muted);
    background: transparent;
  }

  .secondary-action {
    color: var(--text);
    background: var(--surface);
  }

  .primary {
    min-width: 88px;
    border-color: var(--accent);
    color: #ffffff;
    background: var(--accent);
  }

  .primary:disabled {
    border-color: var(--border-strong);
    color: var(--muted);
    background: var(--disabled);
  }

  .payload-view {
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    background: var(--surface);
  }

  .payload-view summary {
    padding: 9px 10px;
    color: var(--text);
    font-size: 12px;
    font-weight: 800;
    cursor: pointer;
  }

  pre {
    max-height: 210px;
    margin: 0;
    border-top: 1px solid var(--border-subtle);
    padding: 10px;
    overflow: auto;
    color: var(--text);
    background: var(--code-bg);
    font-size: 11px;
    line-height: 1.45;
    white-space: pre-wrap;
  }

  .status-text {
    font-size: 12px;
  }

  .error-text {
    color: var(--danger);
    font-weight: 750;
  }

  @media (max-width: 520px) {
    .modal-layer {
      align-items: end;
      padding: 8px;
    }

    .consent-dialog {
      max-width: calc(100vw - 16px);
    }
  }
</style>
