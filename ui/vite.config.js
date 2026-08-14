import { defineConfig } from 'vite';
import { readFileSync } from 'node:fs';

const cargoManifest = readFileSync(new URL('../Cargo.toml', import.meta.url), 'utf8');
const workspacePackage = cargoManifest.match(/\[workspace\.package\]([\s\S]*?)(?=\r?\n\[|$)/)?.[1];
const appVersion = workspacePackage?.match(/^\s*version\s*=\s*"([^"]+)"/m)?.[1];
if (!appVersion) throw new Error('Cargo.toml の [workspace.package].version が見つかりません');

export default defineConfig({
  base: './',
  plugins: [{
    name: 'cargo-version',
    transformIndexHtml(html) {
      return html.replaceAll('__APP_VERSION__', appVersion);
    },
  }],
  build: {
    target: 'es2022',
    outDir: 'dist',
    emptyOutDir: true,
    sourcemap: false,
  },
});
