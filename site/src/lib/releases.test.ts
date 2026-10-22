import { describe, expect, test } from 'bun:test';
import { RELEASES_URL } from '../consts';
import {
  type GitHubRelease,
  compareVersions,
  fetchDownloads,
  matchAssets,
  parseTag,
  pickReleases,
} from './releases';

const ALL_ASSETS = [
  'Tandem-{v}-1.x86_64.rpm',
  'Tandem_{v}_aarch64.app.tar.gz',
  'Tandem_{v}_aarch64.dmg',
  'Tandem_{v}_amd64.AppImage',
  'Tandem_{v}_amd64.deb',
  'Tandem_{v}_x64-setup.exe',
  'Tandem_{v}_x64.app.tar.gz',
  'Tandem_{v}_x64.dmg',
  'Tandem_{v}_x64_en-US.msi',
];

function release(tag: string, opts: { draft?: boolean; assets?: string[] } = {}): GitHubRelease {
  const v = tag.slice(1);
  const page = `https://github.com/LaszloMocsy/tandem/releases/tag/${tag}`;
  return {
    tag_name: tag,
    draft: opts.draft ?? false,
    html_url: page,
    published_at: '2026-10-16T11:44:32Z',
    assets: (opts.assets ?? ALL_ASSETS).map((pattern) => {
      const name = pattern.replace('{v}', v);
      return { name, browser_download_url: `https://example.test/${tag}/${name}` };
    }),
  };
}

describe('parseTag', () => {
  test('classifies production, beta and alpha tags', () => {
    expect(parseTag('v1.2.3')).toMatchObject({ channel: 'production', major: 1, minor: 2, patch: 3, text: '1.2.3' });
    expect(parseTag('v0.2.0-beta.4')).toMatchObject({ channel: 'beta', pre: 4, text: '0.2.0-beta.4' });
    expect(parseTag('v0.2.0-alpha.1')).toMatchObject({ channel: 'alpha', pre: 1, text: '0.2.0-alpha.1' });
  });

  test('ignores tags that match no channel', () => {
    for (const tag of [
      '1.2.3',
      'v1.2',
      'v1.2.3.4',
      'v1.2.3-rc.1',
      'v1.2.3-beta',
      'v1.2.3-beta.1.2',
      'v1.2.3-Beta.1',
      'nightly',
      'tandem-v1.2.3',
      'v1.2.3 ',
      '',
    ]) {
      expect(parseTag(tag)).toBeNull();
    }
  });
});

describe('compareVersions', () => {
  const v = (tag: string) => parseTag(tag)!;

  test('orders numerically, not as text', () => {
    expect(compareVersions(v('v0.10.0'), v('v0.9.0'))).toBeGreaterThan(0);
    expect(compareVersions(v('v0.2.0-beta.10'), v('v0.2.0-beta.9'))).toBeGreaterThan(0);
  });

  test('puts pre-releases before their production version, alpha before beta', () => {
    expect(compareVersions(v('v0.2.0-beta.1'), v('v0.2.0'))).toBeLessThan(0);
    expect(compareVersions(v('v0.2.0-alpha.5'), v('v0.2.0-beta.1'))).toBeLessThan(0);
    expect(compareVersions(v('v0.2.0-alpha.1'), v('v0.1.9'))).toBeGreaterThan(0);
  });
});

describe('pickReleases', () => {
  test('keeps the newest release per channel, whatever the list order', () => {
    const d = pickReleases([
      release('v0.1.0'),
      release('v0.3.0-beta.1'),
      release('v0.2.0'),
      release('v0.3.0-beta.2'),
      release('v0.4.0-alpha.3'),
      release('v0.4.0-alpha.12'),
      release('v0.1.5'),
    ]);
    expect(d.production?.version).toBe('0.2.0');
    expect(d.beta?.version).toBe('0.3.0-beta.2');
    expect(d.alpha?.version).toBe('0.4.0-alpha.12');
    expect(d.hasProduction).toBe(true);
    expect(d.fallback).toBe(false);
  });

  test('hides beta and alpha that are not newer than production', () => {
    const d = pickReleases([
      release('v0.3.0'),
      release('v0.3.0-beta.2'),
      release('v0.2.0-alpha.9'),
    ]);
    expect(d.production?.version).toBe('0.3.0');
    expect(d.beta).toBeNull();
    expect(d.alpha).toBeNull();
  });

  test('keeps pre-releases newer than production and hides older ones', () => {
    const d = pickReleases([release('v0.3.0'), release('v0.4.0-beta.1'), release('v0.3.0-alpha.2')]);
    expect(d.beta?.version).toBe('0.4.0-beta.1');
    expect(d.alpha).toBeNull();
  });

  test('without a production release, reports none and keeps pre-releases', () => {
    const d = pickReleases([release('v0.2.0-beta.1'), release('v0.2.0-alpha.3')]);
    expect(d.production).toBeNull();
    expect(d.hasProduction).toBe(false);
    expect(d.beta?.version).toBe('0.2.0-beta.1');
    expect(d.alpha?.version).toBe('0.2.0-alpha.3');
  });

  test('ignores drafts and tags that match no channel', () => {
    const d = pickReleases([
      release('v0.1.0'),
      release('v0.9.0', { draft: true }),
      release('v0.8.0-rc.1'),
      release('nightly'),
    ]);
    expect(d.production?.version).toBe('0.1.0');
    expect(d.beta).toBeNull();
    expect(d.alpha).toBeNull();
  });

  test('exposes the tag, date and release page', () => {
    const d = pickReleases([release('v1.0.0')]);
    expect(d.production).toMatchObject({
      channel: 'production',
      tag: 'v1.0.0',
      date: '2026-10-16T11:44:32Z',
      url: 'https://github.com/LaszloMocsy/tandem/releases/tag/v1.0.0',
    });
  });

  test('an empty list has no releases', () => {
    const d = pickReleases([]);
    expect(d).toMatchObject({ production: null, beta: null, alpha: null, hasProduction: false });
  });
});

describe('matchAssets', () => {
  test('matches every OS file by pattern', () => {
    const files = matchAssets(release('v2.3.4'));
    const name = (a: { name: string | null }) => a.name;
    expect(name(files.macos.main)).toBe('Tandem_2.3.4_aarch64.dmg');
    expect(name(files.macos.intel)).toBe('Tandem_2.3.4_x64.dmg');
    expect(name(files.windows.main)).toBe('Tandem_2.3.4_x64-setup.exe');
    expect(name(files.linux.main)).toBe('Tandem_2.3.4_amd64.AppImage');
    expect(name(files.linux.deb)).toBe('Tandem_2.3.4_amd64.deb');
    expect(name(files.linux.rpm)).toBe('Tandem-2.3.4-1.x86_64.rpm');
    expect(files.windows.main.url).toBe('https://example.test/v2.3.4/Tandem_2.3.4_x64-setup.exe');
  });

  test('does not mistake the .app.tar.gz or .msi for a main file', () => {
    const files = matchAssets(
      release('v1.0.0', { assets: ['Tandem_{v}_aarch64.app.tar.gz', 'Tandem_{v}_x64_en-US.msi'] }),
    );
    expect(files.macos.main.name).toBeNull();
    expect(files.windows.main.name).toBeNull();
  });

  test('falls back to the release page for a missing file', () => {
    const r = release('v1.0.0', { assets: ALL_ASSETS.filter((a) => !a.endsWith('.rpm') && !a.endsWith('_x64.dmg')) });
    const files = matchAssets(r);
    expect(files.linux.rpm).toEqual({ name: null, url: r.html_url });
    expect(files.macos.intel).toEqual({ name: null, url: r.html_url });
    expect(files.linux.deb.name).toBe('Tandem_1.0.0_amd64.deb');
  });
});

describe('fetchDownloads', () => {
  const ok = (body: unknown) =>
    (async () => new Response(JSON.stringify(body), { status: 200 })) as unknown as typeof fetch;

  function expectFallback(d: Awaited<ReturnType<typeof fetchDownloads>>) {
    expect(d.fallback).toBe(true);
    expect(d.hasProduction).toBe(true);
    expect(d.beta).toBeNull();
    expect(d.alpha).toBeNull();
    const f = d.production!.files;
    for (const asset of [f.macos.main, f.macos.intel, f.windows.main, f.linux.main, f.linux.deb, f.linux.rpm]) {
      expect(asset).toEqual({ name: null, url: RELEASES_URL });
    }
  }

  test('builds download data from the API response', async () => {
    const d = await fetchDownloads({ fetch: ok([release('v1.0.0')]), token: '' });
    expect(d.fallback).toBe(false);
    expect(d.production?.version).toBe('1.0.0');
  });

  test('sends the token when one is given', async () => {
    let auth = null as string | null;
    const spy = (async (_url: string, init: RequestInit) => {
      auth = new Headers(init.headers).get('Authorization');
      return new Response('[]');
    }) as unknown as typeof fetch;
    await fetchDownloads({ fetch: spy, token: 'secret' });
    expect(auth).toBe('Bearer secret');
  });

  test('falls back when the request fails', async () => {
    const fail = (async () => {
      throw new TypeError('network down');
    }) as unknown as typeof fetch;
    expectFallback(await fetchDownloads({ fetch: fail, token: '' }));
  });

  test('falls back on an error status', async () => {
    const limited = (async () => new Response('rate limited', { status: 403 })) as unknown as typeof fetch;
    expectFallback(await fetchDownloads({ fetch: limited, token: '' }));
  });

  test('falls back on a body that is not a release list', async () => {
    expectFallback(await fetchDownloads({ fetch: ok({ message: 'Not Found' }), token: '' }));
    const broken = (async () => new Response('not json')) as unknown as typeof fetch;
    expectFallback(await fetchDownloads({ fetch: broken, token: '' }));
    expectFallback(await fetchDownloads({ fetch: ok([{ tag_name: 'v1.0.0' }]), token: '' }));
  });
});
