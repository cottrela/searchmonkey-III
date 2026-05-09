export type ReleaseAsset = {
  name: string;
  browser_download_url: string;
};

export type LatestReleaseResponse = {
  release?: {
    tag_name?: string;
    html_url?: string;
    assets?: ReleaseAsset[];
  };
};

export type AvailableUpdate = {
  currentVersion: string;
  tagName: string;
  releaseUrl: string;
  downloadUrl: string;
  downloadName: string;
};

const LATEST_RELEASE_ENDPOINT = 'https://searchmonkey.dev/api/latest-release';

export async function getAvailableUpdate(
  currentVersion: string,
  fetcher: typeof fetch = fetch
): Promise<AvailableUpdate | null> {
  const response = await fetcher(LATEST_RELEASE_ENDPOINT, {
    method: 'GET',
    headers: { accept: 'application/json' }
  });

  if (!response.ok) {
    throw new Error(`Latest release check failed with ${response.status}`);
  }

  const data = (await response.json()) as LatestReleaseResponse;
  const release = data.release;
  const tagName = release?.tag_name?.trim();

  if (!release || !tagName || compareVersions(tagName, currentVersion) <= 0) {
    return null;
  }

  const asset = selectBestAsset(release.assets ?? []);
  const releaseUrl = release.html_url ?? 'https://github.com/cottrela/searchmonkey-III/releases';

  return {
    currentVersion,
    tagName,
    releaseUrl,
    downloadUrl: asset?.browser_download_url ?? releaseUrl,
    downloadName: asset?.name ?? 'release page'
  };
}

export function compareVersions(left: string, right: string) {
  const leftVersion = parseVersion(left);
  const rightVersion = parseVersion(right);

  if (!leftVersion || !rightVersion) return 0;

  const length = Math.max(leftVersion.parts.length, rightVersion.parts.length);
  for (let index = 0; index < length; index += 1) {
    const difference = (leftVersion.parts[index] ?? 0) - (rightVersion.parts[index] ?? 0);
    if (difference !== 0) return difference;
  }

  if (leftVersion.prerelease && !rightVersion.prerelease) return -1;
  if (!leftVersion.prerelease && rightVersion.prerelease) return 1;
  if (leftVersion.prerelease && rightVersion.prerelease) {
    return leftVersion.prerelease.localeCompare(rightVersion.prerelease, undefined, {
      numeric: true,
      sensitivity: 'base'
    });
  }

  return 0;
}

function parseVersion(version: string) {
  const match = version.trim().match(/^v?(\d+(?:\.\d+)*)(?:-([0-9A-Za-z.-]+))?(?:\+.+)?$/);
  if (!match) return null;

  return {
    parts: match[1].split('.').map((part) => Number(part)),
    prerelease: match[2] ?? ''
  };
}

function selectBestAsset(assets: ReleaseAsset[]) {
  if (!assets.length) return null;

  const platform = navigator.platform.toLowerCase();
  const userAgent = navigator.userAgent.toLowerCase();
  const isArm = platform.includes('arm') || userAgent.includes('arm64') || userAgent.includes('aarch64');
  const target = platform.includes('mac')
    ? ['mac', 'darwin', 'apple', 'dmg']
    : platform.includes('win')
      ? ['windows', 'win', 'msi', 'exe']
      : ['linux', 'appimage', 'deb', 'rpm', 'tar.gz'];
  const arch = isArm ? ['arm64', 'aarch64'] : ['x64', 'x86_64', 'amd64'];

  return (
    bestAssetMatching(assets, target, arch) ??
    bestAssetMatching(assets, target, []) ??
    bestAssetMatching(assets, [], arch) ??
    assets[0]
  );
}

function bestAssetMatching(assets: ReleaseAsset[], targetTokens: string[], archTokens: string[]) {
  const candidates = assets
    .map((asset) => ({
      asset,
      name: asset.name.toLowerCase()
    }))
    .filter(({ name }) => targetTokens.length === 0 || targetTokens.some((token) => name.includes(token)))
    .filter(({ name }) => archTokens.length === 0 || archTokens.some((token) => name.includes(token)));

  return candidates.find(({ name }) => name.endsWith('.dmg') || name.endsWith('.msi') || name.endsWith('.appimage'))?.asset
    ?? candidates.find(({ name }) => name.endsWith('.deb') || name.endsWith('.rpm') || name.endsWith('.exe'))?.asset
    ?? candidates[0]?.asset
    ?? null;
}
