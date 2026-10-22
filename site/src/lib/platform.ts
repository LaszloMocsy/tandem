// Picks the download tab for the visitor's system. Runs in the browser.

export type Os = 'macos' | 'windows' | 'linux';

export interface PlatformInfo {
  userAgent: string;
  maxTouchPoints: number;
  /** From navigator.userAgentData, where the browser supports it. */
  uaData?: { platform: string; mobile: boolean };
}

/** The visitor's desktop OS, or null for phones, tablets and systems Tandem doesn't ship for. */
export function detectOs({ userAgent, maxTouchPoints, uaData }: PlatformInfo): Os | null {
  const ua = userAgent.toLowerCase();
  if (uaData?.mobile ?? /android|iphone|ipad|ipod|mobile/.test(ua)) return null;
  const platform = (uaData?.platform || ua).toLowerCase();
  // iPadOS reports itself as a Mac; touch support gives it away.
  if (platform.includes('mac')) return maxTouchPoints > 1 ? null : 'macos';
  if (platform.includes('win')) return 'windows';
  if (platform.includes('cros')) return null;
  if (platform.includes('linux') || platform.includes('x11')) return 'linux';
  return null;
}
