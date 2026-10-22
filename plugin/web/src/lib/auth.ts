/** Token authentication for the web dashboard.
 *
 *  When the daemon is configured with `SDI_REQUIRE_TOKEN=1`, every HTTP
 *  request must carry `X-Sdi-Token: <secret>`. The user supplies the token
 *  once via localStorage; this module is the single read/write surface.
 *
 *  Storage key: `sdi.auth.token` — distinct from theme, drawer, etc.
 *  The token never leaves localStorage (no analytics, no error reports).
 */

const KEY = 'sdi.auth.token';

export function getStoredToken(): string | null {
  try {
    const raw = localStorage.getItem(KEY);
    return raw && raw.length > 0 ? raw : null;
  } catch {
    return null;
  }
}

export function setStoredToken(token: string | null): void {
  try {
    if (token && token.length > 0) localStorage.setItem(KEY, token);
    else localStorage.removeItem(KEY);
  } catch { /* private mode / quota — ignore */ }
}

/** Build the auth headers slice. Empty when no token is configured so the
 *  network layer can spread it without overwriting other headers. */
export function authHeaders(): Record<string, string> {
  const t = getStoredToken();
  return t ? { 'X-Sdi-Token': t } : {};
}
