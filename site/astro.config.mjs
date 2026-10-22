// @ts-check
import { defineConfig } from 'astro/config';
import sitemap from '@astrojs/sitemap';

// Static output only. Starlight will mount at /docs later (#10), so keep
// that path free.
export default defineConfig({
  site: 'https://tandem.laszlomocsy.com',
  output: 'static',
  trailingSlash: 'ignore',
  integrations: [sitemap()],
});
