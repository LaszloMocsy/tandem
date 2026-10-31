// Turns the repo's GitHub releases into download data for the page. Runs at
// build time only; the site is redeployed when a release is published.
import { RELEASES_URL } from '../consts';

const RELEASES_API = 'https://api.github.com/repos/LaszloMocsy/tandem/releases?per_page=100';

export type Channel = 'production' | 'beta';

/** A parsed release tag: vX.Y.Z for production, vX.Y.Z-beta.N for a beta. */
export interface Version {
  channel: Channel;
  major: number;
  minor: number;
  patch: number;
  /** The N in -beta.N; 0 for production. */
  pre: number;
  /** The tag without its leading v, such as 0.2.0-beta.1. */
  text: string;
}

/** A download link. `name` is null when the release had no matching file and `url` is its release page. */
export interface Asset {
  name: string | null;
  url: string;
}

export interface Files {
  macos: { main: Asset; intel: Asset };
  windows: { main: Asset };
  linux: { main: Asset; deb: Asset; rpm: Asset };
}

export interface ChannelRelease {
  channel: Channel;
  /** Null only in fallback mode, when releases couldn't be fetched. */
  version: string | null;
  tag: string | null;
  /** ISO 8601 publish date, or null in fallback mode. */
  date: string | null;
  url: string;
  files: Files;
}

export interface Downloads {
  production: ChannelRelease | null;
  /** Only set when newer than the production release. */
  beta: ChannelRelease | null;
  hasProduction: boolean;
  /** True when releases couldn't be fetched and every link points at the releases page. */
  fallback: boolean;
  releasesUrl: string;
}

/** The fields we read from GitHub's release objects. */
export interface GitHubRelease {
  tag_name: string;
  draft: boolean;
  html_url: string;
  published_at: string | null;
  /** The release notes, in Markdown. */
  body?: string | null;
  assets: { name: string; browser_download_url: string }[];
}

const TAG = /^v(\d+)\.(\d+)\.(\d+)(?:-(beta)\.(\d+))?$/;

/** Classifies a tag by channel, or returns null for tags that match none. */
export function parseTag(tag: string): Version | null {
  const m = TAG.exec(tag);
  if (!m) return null;
  return {
    channel: (m[4] as Channel | undefined) ?? 'production',
    major: Number(m[1]),
    minor: Number(m[2]),
    patch: Number(m[3]),
    pre: m[5] === undefined ? 0 : Number(m[5]),
    text: tag.slice(1),
  };
}

const CHANNEL_RANK: Record<Channel, number> = { beta: 0, production: 1 };

/** Semver order: a beta sorts before its own production version. */
export function compareVersions(a: Version, b: Version): number {
  return (
    a.major - b.major ||
    a.minor - b.minor ||
    a.patch - b.patch ||
    CHANNEL_RANK[a.channel] - CHANNEL_RANK[b.channel] ||
    a.pre - b.pre
  );
}

// Matched by suffix so file names never need the version.
const PATTERNS = {
  macos: { main: '_aarch64.dmg', intel: '_x64.dmg' },
  windows: { main: '-setup.exe' },
  linux: { main: '.AppImage', deb: '.deb', rpm: '.rpm' },
} as const;

/** Picks each OS's files from a release's assets, falling back to the release page for any that are missing. */
export function matchAssets(release: GitHubRelease): Files {
  const find = (suffix: string): Asset => {
    const asset = release.assets.find((a) => a.name.endsWith(suffix));
    return asset
      ? { name: asset.name, url: asset.browser_download_url }
      : { name: null, url: release.html_url };
  };
  return {
    macos: { main: find(PATTERNS.macos.main), intel: find(PATTERNS.macos.intel) },
    windows: { main: find(PATTERNS.windows.main) },
    linux: {
      main: find(PATTERNS.linux.main),
      deb: find(PATTERNS.linux.deb),
      rpm: find(PATTERNS.linux.rpm),
    },
  };
}

/** Keeps the newest release per channel, and drops the beta unless it's newer than production. */
export function pickReleases(releases: GitHubRelease[]): Downloads {
  const newest: Partial<Record<Channel, { version: Version; release: GitHubRelease }>> = {};
  for (const release of releases) {
    if (release.draft) continue;
    const version = parseTag(release.tag_name);
    if (!version) continue;
    const current = newest[version.channel];
    if (!current || compareVersions(version, current.version) > 0) {
      newest[version.channel] = { version, release };
    }
  }

  const prod = newest.production;
  const toChannel = (channel: Channel): ChannelRelease | null => {
    const entry = newest[channel];
    if (!entry) return null;
    if (channel !== 'production' && prod && compareVersions(entry.version, prod.version) <= 0) {
      return null;
    }
    return {
      channel,
      version: entry.version.text,
      tag: entry.release.tag_name,
      date: entry.release.published_at,
      url: entry.release.html_url,
      files: matchAssets(entry.release),
    };
  };

  return {
    production: toChannel('production'),
    beta: toChannel('beta'),
    hasProduction: prod !== undefined,
    fallback: false,
    releasesUrl: RELEASES_URL,
  };
}

/** Download data that sends every link to the releases page. */
export function fallbackDownloads(): Downloads {
  const page: Asset = { name: null, url: RELEASES_URL };
  return {
    production: {
      channel: 'production',
      version: null,
      tag: null,
      date: null,
      url: RELEASES_URL,
      files: {
        macos: { main: page, intel: page },
        windows: { main: page },
        linux: { main: page, deb: page, rpm: page },
      },
    },
    beta: null,
    hasProduction: true,
    fallback: true,
    releasesUrl: RELEASES_URL,
  };
}

export interface FetchOptions {
  fetch?: typeof fetch;
  token?: string;
}

/** Fetches the repo's releases. Throws on any failure. */
export async function fetchReleases(options: FetchOptions = {}): Promise<GitHubRelease[]> {
  const doFetch = options.fetch ?? fetch;
  const token = options.token ?? process.env.GITHUB_TOKEN;
  const headers: Record<string, string> = {
    Accept: 'application/vnd.github+json',
    'X-GitHub-Api-Version': '2022-11-28',
  };
  if (token) headers.Authorization = `Bearer ${token}`;

  const res = await doFetch(RELEASES_API, { headers });
  if (!res.ok) throw new Error(`GitHub responded ${res.status}`);
  const body: unknown = await res.json();
  if (!Array.isArray(body)) throw new Error('GitHub returned something other than a list');
  return body as GitHubRelease[];
}

/** Fetches releases and builds download data. Never throws: any failure falls back to the releases page. */
export async function fetchDownloads(options: FetchOptions = {}): Promise<Downloads> {
  try {
    return pickReleases(await fetchReleases(options));
  } catch (err) {
    console.warn(`[releases] Falling back to the releases page: ${String(err)}`);
    return fallbackDownloads();
  }
}

let cachedReleases: Promise<GitHubRelease[]> | undefined;

/** fetchReleases, fetched once per build so the page and the update feeds see the same releases. */
export function getReleases(): Promise<GitHubRelease[]> {
  cachedReleases ??= fetchReleases();
  return cachedReleases;
}

let cached: Promise<Downloads> | undefined;

/** Download data, built once per build so every component sees the same data. */
export function getDownloads(): Promise<Downloads> {
  cached ??= getReleases().then(pickReleases, (err: unknown) => {
    console.warn(`[releases] Falling back to the releases page: ${String(err)}`);
    return fallbackDownloads();
  });
  return cached;
}

// Update feeds. The app checks one of these on start: stable for production
// builds, beta for beta builds, unless Settings picks the other.

export type UpdateChannel = 'stable' | 'beta';

export const UPDATE_CHANNELS: UpdateChannel[] = ['stable', 'beta'];

/** The release channels each update channel offers. */
const OFFERS: Record<UpdateChannel, Channel[]> = {
  stable: ['production'],
  beta: ['production', 'beta'],
};

/** The updater manifest tauri-action uploads to each release. */
export const MANIFEST = 'latest.json';

/** Where release.yml's notes switch from the download note to the changes. */
export const CHANGES_MARKER = '<!-- changes -->';

/** A feed that offers nothing: no app is older than 0.0.0. */
export const NO_UPDATE = { version: '0.0.0', platforms: {} };

/** The newest published release `channel` offers that has an updater manifest, or null. */
export function pickUpdate(releases: GitHubRelease[], channel: UpdateChannel): GitHubRelease | null {
  let newest: { version: Version; release: GitHubRelease } | null = null;
  for (const release of releases) {
    if (release.draft || !release.assets.some((a) => a.name === MANIFEST)) continue;
    const version = parseTag(release.tag_name);
    if (!version || !OFFERS[channel].includes(version.channel)) continue;
    if (!newest || compareVersions(version, newest.version) > 0) newest = { version, release };
  }
  return newest?.release ?? null;
}

/** A release's changes: its notes after the download note, or all of them without the marker. */
export function releaseChanges(body: string | null | undefined): string {
  const text = body ?? '';
  const at = text.indexOf(CHANGES_MARKER);
  return (at === -1 ? text : text.slice(at + CHANGES_MARKER.length)).trim();
}

/**
 * The feed for `channel`: the manifest of the newest release it offers,
 * with the release's changes as its notes, or NO_UPDATE when no release
 * fits. Throws when the manifest can't be fetched.
 */
export async function updateFeed(
  releases: GitHubRelease[],
  channel: UpdateChannel,
  options: Pick<FetchOptions, 'fetch'> = {},
): Promise<Record<string, unknown>> {
  const release = pickUpdate(releases, channel);
  if (!release) return NO_UPDATE;
  const asset = release.assets.find((a) => a.name === MANIFEST)!;
  const res = await (options.fetch ?? fetch)(asset.browser_download_url);
  if (!res.ok) throw new Error(`${release.tag_name}'s ${MANIFEST} responded ${res.status}`);
  const manifest: unknown = await res.json();
  if (typeof manifest !== 'object' || manifest === null || Array.isArray(manifest)) {
    throw new Error(`${release.tag_name}'s ${MANIFEST} is not an object`);
  }
  return { ...manifest, notes: releaseChanges(release.body) };
}
