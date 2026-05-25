/** Theme management — persists to localStorage, syncs data-theme attribute.
 *  Listens to OS prefers-color-scheme to set initial value when unset.
 *
 *  - localStorage key: `sdi.theme`
 *  - values: `light` | `dark` | `system`
 *  - default when unset: `system`
 */

export type Theme = 'dark' | 'light' | 'system';

const KEY = 'sdi.theme';

function applyTheme(theme: Theme): void {
  const root = document.documentElement;
  if (theme === 'system') {
    const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
    root.setAttribute('data-theme', prefersDark ? 'dark' : 'light');
  } else {
    root.setAttribute('data-theme', theme);
  }
}

export function getStoredTheme(): Theme {
  const stored = localStorage.getItem(KEY) as Theme | null;
  if (stored === 'dark' || stored === 'light' || stored === 'system') return stored;
  return 'system';
}

export function setTheme(theme: Theme): void {
  localStorage.setItem(KEY, theme);
  applyTheme(theme);
}

/** Call once on app mount. Returns a cleanup fn that removes the OS listener. */
export function initTheme(): () => void {
  const theme = getStoredTheme();
  applyTheme(theme);

  const mq = window.matchMedia('(prefers-color-scheme: dark)');
  const handleChange = () => {
    if (getStoredTheme() === 'system') applyTheme('system');
  };
  mq.addEventListener('change', handleChange);
  return () => mq.removeEventListener('change', handleChange);
}

export function getCurrentEffectiveTheme(): 'dark' | 'light' {
  const stored = getStoredTheme();
  if (stored !== 'system') return stored;
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}
