import { getVersion } from '@tauri-apps/api/app';

export type TelemetryConsent = 'yes' | 'no';

export type TelemetryState = {
  installId: string;
  consent: TelemetryConsent | null;
  prompted: boolean;
  lastSubmittedConsent: TelemetryConsent | null;
  lastSubmittedAt: string | null;
  lastError: string | null;
};

export type TelemetryPayload = {
  install_id: string;
  consent: TelemetryConsent;
  app_version?: string;
  platform?: string;
  platform_version?: string;
  arch?: string;
  screen_size?: string;
  locale?: string;
};

const TELEMETRY_CONSENT_ENDPOINT = 'https://searchmonkey.dev/api/telemetry/consent';
const INSTALL_ID_KEY = 'searchmonkey:install-id';
const CONSENT_KEY = 'searchmonkey:telemetry-consent';
const PROMPTED_KEY = 'searchmonkey:telemetry-consent-prompted';
const LAST_SUBMITTED_CONSENT_KEY = 'searchmonkey:telemetry-last-submitted-consent';
const LAST_SUBMITTED_AT_KEY = 'searchmonkey:telemetry-last-submitted-at';

export function loadTelemetryState(): TelemetryState {
  return {
    installId: getOrCreateInstallId(),
    consent: parseConsent(localStorage.getItem(CONSENT_KEY)),
    prompted: localStorage.getItem(PROMPTED_KEY) === 'true',
    lastSubmittedConsent: parseConsent(localStorage.getItem(LAST_SUBMITTED_CONSENT_KEY)),
    lastSubmittedAt: localStorage.getItem(LAST_SUBMITTED_AT_KEY),
    lastError: null
  };
}

export async function saveTelemetryConsent(consent: TelemetryConsent): Promise<TelemetryState> {
  const current = loadTelemetryState();
  localStorage.setItem(CONSENT_KEY, consent);
  localStorage.setItem(PROMPTED_KEY, 'true');

  return syncTelemetryConsent({
    ...current,
    consent,
    prompted: true,
    lastError: null
  });
}

export async function syncTelemetryConsent(state = loadTelemetryState()): Promise<TelemetryState> {
  if (!state.consent || state.lastSubmittedConsent === state.consent) {
    return state;
  }

  try {
    await submitTelemetryConsent(await buildTelemetryPayload(state.installId, state.consent));
  } catch (error) {
    return {
      ...state,
      lastError: error instanceof Error ? error.message : 'Could not sync telemetry preference.'
    };
  }

  const submittedAt = new Date().toISOString();
  localStorage.setItem(LAST_SUBMITTED_CONSENT_KEY, state.consent);
  localStorage.setItem(LAST_SUBMITTED_AT_KEY, submittedAt);

  return {
    ...state,
    lastSubmittedConsent: state.consent,
    lastSubmittedAt: submittedAt,
    lastError: null
  };
}

export async function previewTelemetryPayload(consent: TelemetryConsent): Promise<TelemetryPayload> {
  return buildTelemetryPayload(getOrCreateInstallId(), consent);
}

function getOrCreateInstallId() {
  const existing = localStorage.getItem(INSTALL_ID_KEY);
  if (existing && isUuid(existing)) return existing;

  const installId = crypto.randomUUID();
  localStorage.setItem(INSTALL_ID_KEY, installId);
  return installId;
}

async function buildTelemetryPayload(installId: string, consent: TelemetryConsent): Promise<TelemetryPayload> {
  const base: TelemetryPayload = {
    install_id: installId,
    consent,
    app_version: await appVersion(),
    platform: platformName()
  };

  if (consent === 'no') {
    return compactPayload(base);
  }

  return compactPayload({
    ...base,
    platform_version: platformVersion(),
    arch: architecture(),
    screen_size: `${screen.width}x${screen.height}`,
    locale: navigator.language
  });
}

async function submitTelemetryConsent(payload: TelemetryPayload) {
  const response = await fetch(TELEMETRY_CONSENT_ENDPOINT, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(payload)
  });

  if (!response.ok) {
    throw new Error(`Consent endpoint returned ${response.status}.`);
  }
}

async function appVersion() {
  try {
    return await getVersion();
  } catch {
    return undefined;
  }
}

function platformName() {
  const userAgent = navigator.userAgent.toLowerCase();
  if (userAgent.includes('win')) return 'windows';
  if (userAgent.includes('mac')) return 'macos';
  if (userAgent.includes('linux')) return 'linux';
  return navigator.platform.toLowerCase() || 'unknown';
}

function platformVersion() {
  const userAgent = navigator.userAgent;
  const macMatch = userAgent.match(/Mac OS X (\d+(?:[_\.]\d+)*)/i);
  if (macMatch) return macMatch[1].replaceAll('_', '.');

  const windowsMatch = userAgent.match(/Windows NT (\d+(?:\.\d+)*)/i);
  if (windowsMatch) return windowsMatch[1];

  const iosMatch = userAgent.match(/OS (\d+(?:[_\.]\d+)*) like Mac OS X/i);
  if (iosMatch) return iosMatch[1].replaceAll('_', '.');

  const androidMatch = userAgent.match(/Android (\d+(?:\.\d+)*)/i);
  if (androidMatch) return androidMatch[1];

  return undefined;
}

function architecture() {
  const userAgent = navigator.userAgent.toLowerCase();
  const platform = navigator.platform.toLowerCase();

  if (userAgent.includes('arm64') || userAgent.includes('aarch64') || platform.includes('arm')) {
    return 'arm64';
  }

  if (userAgent.includes('x86_64') || userAgent.includes('win64') || userAgent.includes('x64') || platform.includes('x86_64')) {
    return 'x86_64';
  }

  if (userAgent.includes('i686') || userAgent.includes('i386') || userAgent.includes('x86')) {
    return 'x86';
  }

  return undefined;
}

function compactPayload(payload: TelemetryPayload) {
  return Object.fromEntries(Object.entries(payload).filter(([, value]) => value)) as TelemetryPayload;
}

function parseConsent(value: string | null): TelemetryConsent | null {
  return value === 'yes' || value === 'no' ? value : null;
}

function isUuid(value: string) {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value);
}
