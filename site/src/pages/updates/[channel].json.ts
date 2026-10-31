// The app's update feeds, /updates/stable.json and /updates/beta.json,
// built from the GitHub releases like the download links. The site is
// redeployed when a release is published, which updates them.
//
// When the releases can't be fetched, the feeds are left out, so the app's
// check fails and says so rather than finding nothing. That beats a feed
// that wrongly offers nothing.
import type { APIRoute, GetStaticPaths } from 'astro';
import { getReleases, UPDATE_CHANNELS, updateFeed } from '../../lib/releases';

export const getStaticPaths = (async () => {
  let releases;
  try {
    releases = await getReleases();
  } catch (err) {
    console.warn(`[updates] Leaving the update feeds out: ${String(err)}`);
    return [];
  }
  const paths = [];
  for (const channel of UPDATE_CHANNELS) {
    try {
      paths.push({ params: { channel }, props: { feed: await updateFeed(releases, channel) } });
    } catch (err) {
      console.warn(`[updates] Leaving the ${channel} feed out: ${String(err)}`);
    }
  }
  return paths;
}) satisfies GetStaticPaths;

export const GET: APIRoute = ({ props }) =>
  new Response(JSON.stringify(props.feed), {
    headers: { 'Content-Type': 'application/json' },
  });
