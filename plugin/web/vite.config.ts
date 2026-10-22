import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import { readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';

// SDI dashboard build target.
//
// `pnpm build` emits to `../../plugin/web/dist/` so the install gate's
// `ensureWebBundle` step can copy that directory under `pluginRoot/web/`.
//
// Dev mode proxies bare daemon paths (no `/api` rewrite — daemon serves
// them at root). `/events` (SSE) is intentionally NOT proxied: Vite's proxy
// buffers `text/event-stream`, which keeps EventSource stuck in CONNECTING.
// Instead we inject `__SDI_DAEMON_URL__` at dev time so the EventSource
// connects to the daemon origin directly. In production the daemon serves
// the SPA itself, so the constant collapses to an empty string and
// `/events` becomes same-origin.

function readPortFile(): string | null {
  const candidates = [
    process.env.SDI_CACHE_DIR && join(process.env.SDI_CACHE_DIR, 'sdid.port'),
    process.env.XDG_CACHE_HOME && join(process.env.XDG_CACHE_HOME, 'sdi/sdid.port'),
    join(homedir(), '.cache/sdi/sdid.port'),
  ].filter((p): p is string => Boolean(p));
  for (const path of candidates) {
    try {
      const raw = readFileSync(path, 'utf8').trim();
      if (/^\d+$/.test(raw)) return raw;
    } catch {
      // missing / unreadable — try next.
    }
  }
  return null;
}

function getDaemonUrl(): string {
  if (process.env.SDI_DAEMON_URL) return process.env.SDI_DAEMON_URL;
  if (process.env.SDI_WEB_DEV_DAEMON) {
    const v = process.env.SDI_WEB_DEV_DAEMON;
    if (/^\d+$/.test(v)) return `http://127.0.0.1:${v}`;
    return v;
  }
  const fromFile = readPortFile();
  if (fromFile) return `http://127.0.0.1:${fromFile}`;
  return 'http://127.0.0.1:19500';
}

export default defineConfig(({ command }) => {
  const daemonUrl = getDaemonUrl();
  return {
    plugins: [react(), tailwindcss()],
    define: {
      __SDI_DAEMON_URL__: JSON.stringify(command === 'serve' ? daemonUrl : ''),
    },
    build: {
      target: 'es2022',
      sourcemap: true,
      rollupOptions: {
        output: {
          manualChunks: {
            vendor: ['react', 'react-dom', 'react-router-dom'],
            dnd: ['@dnd-kit/core', '@dnd-kit/sortable', '@dnd-kit/utilities'],
            markdown: ['react-markdown', 'remark-gfm'],
          },
        },
      },
    },
    server: {
      port: 5175,
      strictPort: true,
      proxy: {
        '/health':            { target: daemonUrl, changeOrigin: true },
        '/projects':          { target: daemonUrl, changeOrigin: true },
        '/plans':             { target: daemonUrl, changeOrigin: true },
        '/requirements':      { target: daemonUrl, changeOrigin: true },
        '/scenarios':         { target: daemonUrl, changeOrigin: true },
        '/rounds':            { target: daemonUrl, changeOrigin: true },
        '/tasks':             { target: daemonUrl, changeOrigin: true },
        '/knowledge':         { target: daemonUrl, changeOrigin: true },
        '/decisions':         { target: daemonUrl, changeOrigin: true },
        '/disruption-reviews':{ target: daemonUrl, changeOrigin: true },
        '/patterns':          { target: daemonUrl, changeOrigin: true },
        '/autonomy_policies': { target: daemonUrl, changeOrigin: true },
        '/agent_notes':       { target: daemonUrl, changeOrigin: true },
        // /events is intentionally NOT proxied (SSE chunks get buffered).
      },
    },
  };
});
