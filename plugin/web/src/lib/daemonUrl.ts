// Vite injects __SDI_DAEMON_URL__ at build time (see vite.config.ts).
// In production the daemon serves the web bundle, so same-origin works and
// the constant is left empty. In dev the constant points to the daemon
// (e.g. http://127.0.0.1:19500), bypassing the proxy for endpoints that
// don't survive its buffering — chiefly /events (SSE).
declare const __SDI_DAEMON_URL__: string;

const RAW: string =
  typeof __SDI_DAEMON_URL__ === 'string' ? __SDI_DAEMON_URL__ : '';

/** Absolute daemon origin in dev, empty string in prod. No trailing slash. */
export const DAEMON_ORIGIN: string = RAW.replace(/\/+$/, '');

/** Build a daemon URL for an endpoint that must not pass through the
 *  Vite proxy in dev. Pass the path including the leading slash. */
export function daemonUrl(path: string): string {
  return `${DAEMON_ORIGIN}${path}`;
}
