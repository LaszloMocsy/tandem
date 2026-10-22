import { describe, expect, test } from 'bun:test';
import { detectOs } from './platform';

const UA = {
  macSafari:
    'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15',
  winFirefox: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:140.0) Gecko/20100101 Firefox/140.0',
  linuxFirefox: 'Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0',
  chromeOs: 'Mozilla/5.0 (X11; CrOS x86_64 14541.0.0) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36',
  android: 'Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Mobile Safari/537.36',
  iphone:
    'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1',
  chromeReduced: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36',
};

describe('detectOs', () => {
  test('reads the user agent when userAgentData is missing', () => {
    expect(detectOs({ userAgent: UA.macSafari, maxTouchPoints: 0 })).toBe('macos');
    expect(detectOs({ userAgent: UA.winFirefox, maxTouchPoints: 0 })).toBe('windows');
    expect(detectOs({ userAgent: UA.linuxFirefox, maxTouchPoints: 0 })).toBe('linux');
  });

  test('prefers userAgentData.platform over the user agent', () => {
    const uaData = { platform: 'macOS', mobile: false };
    expect(detectOs({ userAgent: UA.chromeReduced, maxTouchPoints: 0, uaData })).toBe('macos');
    expect(
      detectOs({ userAgent: UA.chromeReduced, maxTouchPoints: 0, uaData: { platform: 'Linux', mobile: false } }),
    ).toBe('linux');
  });

  test('falls back to the user agent when the platform is empty', () => {
    const uaData = { platform: '', mobile: false };
    expect(detectOs({ userAgent: UA.winFirefox, maxTouchPoints: 0, uaData })).toBe('windows');
  });

  test('returns null for phones and tablets', () => {
    expect(detectOs({ userAgent: UA.android, maxTouchPoints: 5 })).toBeNull();
    expect(detectOs({ userAgent: UA.iphone, maxTouchPoints: 5 })).toBeNull();
    expect(
      detectOs({ userAgent: UA.android, maxTouchPoints: 5, uaData: { platform: 'Android', mobile: true } }),
    ).toBeNull();
    // iPadOS sends a Mac user agent; touch points give it away.
    expect(detectOs({ userAgent: UA.macSafari, maxTouchPoints: 5 })).toBeNull();
  });

  test('returns null for ChromeOS and unknown systems', () => {
    expect(detectOs({ userAgent: UA.chromeOs, maxTouchPoints: 0 })).toBeNull();
    expect(
      detectOs({ userAgent: UA.chromeReduced, maxTouchPoints: 0, uaData: { platform: 'Chrome OS', mobile: false } }),
    ).toBeNull();
    expect(detectOs({ userAgent: 'curl/8.0', maxTouchPoints: 0 })).toBeNull();
  });
});
