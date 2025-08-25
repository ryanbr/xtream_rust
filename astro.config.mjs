// @ts-check
import { defineConfig } from 'astro/config';

import tailwindcss from '@tailwindcss/vite';

// import netlify from '@astrojs/netlify';

// https://astro.build/config
export default defineConfig({
  site: 'https://dev.infinitel8p.com',
  base: '/xtream/',
  vite: {
    plugins: [tailwindcss()]
  },
  // output: 'server', 

  // adapter: netlify()
});