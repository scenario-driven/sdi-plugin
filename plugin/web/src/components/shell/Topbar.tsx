import { useState, useEffect, useCallback } from 'react';
import { AppShell } from './AppShell';
import { cn } from '../../lib/cn';
import { getStoredTheme, setTheme, getCurrentEffectiveTheme, type Theme } from '../../lib/theme';

export type ViewId =
  | 'summary'
  | 'next'
  | 'board'
  | 'backlog'
  | 'timeline'
  | 'wiki'
  | 'patterns';

interface ViewMeta {
  id: ViewId;
  label: string;
}

const VIEWS: readonly ViewMeta[] = [
  { id: 'summary', label: 'Summary' },
  { id: 'next', label: 'Next' },
  { id: 'board', label: 'Board' },
  { id: 'backlog', label: 'Backlog' },
  { id: 'timeline', label: 'Timeline' },
  { id: 'wiki', label: 'Wiki' },
  { id: 'patterns', label: 'Patterns' },
];

const THEME_GLYPH: Record<Theme, string> = {
  light: '☀',
  dark: '☾',
  system: '◑',
};

const THEME_LABEL: Record<Theme, string> = {
  light: 'Light',
  dark: 'Dark',
  system: 'System',
};

function useThemeCycle() {
  const [stored, setStored] = useState<Theme>(getStoredTheme);

  useEffect(() => {
    const update = () => setStored(getStoredTheme());
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    mq.addEventListener('change', update);
    window.addEventListener('storage', update);
    return () => {
      mq.removeEventListener('change', update);
      window.removeEventListener('storage', update);
    };
  }, []);

  const cycle = useCallback(() => {
    const order: Theme[] = ['system', 'dark', 'light'];
    const next = order[(order.indexOf(stored) + 1) % order.length];
    setTheme(next);
    setStored(next);
  }, [stored]);

  return { stored, cycle };
}

interface TopbarProps {
  activeView: ViewId;
  onViewChange: (id: ViewId) => void;
  onOpenPalette: () => void;
  daemonHealthy: boolean;
  onReconnect?: () => void;
  /** Count of CollaborationPattern rows whose lifecycle === 'active'. */
  activePatternCount?: number;
  /** True when at least one active pattern has kind === 'direct' — surfaces
   *  the D23 anti-pattern marker as a red dot on the badge. */
  hasDirectAnti?: boolean;
  /** Hook the badge click to navigate to the Patterns view. */
  onOpenPatterns?: () => void;
}

export function Topbar({
  activeView,
  onViewChange,
  onOpenPalette,
  daemonHealthy,
  onReconnect,
  activePatternCount = 0,
  hasDirectAnti = false,
  onOpenPatterns,
}: TopbarProps) {
  const { stored: themePref, cycle: cycleTheme } = useThemeCycle();
  const effective = getCurrentEffectiveTheme();

  return (
    <AppShell.Topbar data-testid="app-topbar">
      <nav
        role="tablist"
        aria-label="Workspace views"
        className="flex items-center gap-1"
      >
        {VIEWS.map((v) => {
          const isActive = v.id === activeView;
          return (
            <button
              key={v.id}
              type="button"
              role="tab"
              aria-selected={isActive}
              data-view={v.id}
              data-active={isActive || undefined}
              onClick={() => onViewChange(v.id)}
              className={cn(
                'rounded-md px-3 py-1.5',
                'text-sm font-medium',
                'transition-colors cursor-pointer',
                isActive
                  ? 'bg-surface-high text-foreground'
                  : 'text-muted hover:text-foreground hover:bg-surface-high/60',
              )}
            >
              {v.label}
            </button>
          );
        })}
      </nav>
      <div className="ml-auto flex items-center gap-2">
        <button
          type="button"
          onClick={onOpenPatterns}
          data-testid="active-patterns-badge"
          data-has-direct={hasDirectAnti || undefined}
          aria-label={`Active patterns: ${activePatternCount}${hasDirectAnti ? ' (direct anti-pattern present)' : ''}`}
          title={
            hasDirectAnti
              ? `Active patterns: ${activePatternCount} — direct anti-pattern in play`
              : `Active patterns: ${activePatternCount}`
          }
          className={cn(
            'inline-flex items-center gap-1.5',
            'rounded-md px-2 py-1',
            'text-xs',
            'text-muted hover:text-foreground hover:bg-surface-high/60',
            'transition-colors cursor-pointer',
          )}
        >
          <span aria-hidden className="relative inline-flex items-center justify-center">
            <span className="font-mono">⌬</span>
            {hasDirectAnti && (
              <span
                aria-hidden
                className="absolute -top-0.5 -right-1.5 h-1.5 w-1.5 rounded-full bg-danger"
              />
            )}
          </span>
          <span>Active patterns: <span className="font-mono text-foreground">{activePatternCount}</span></span>
        </button>
        <button
          type="button"
          aria-live="polite"
          data-testid="daemon-health"
          data-healthy={daemonHealthy}
          onClick={daemonHealthy ? undefined : onReconnect}
          title={daemonHealthy ? 'Daemon connected' : 'Daemon down — click to reconnect'}
          className={cn(
            'inline-flex items-center gap-1.5',
            'rounded-md px-2 py-1',
            'text-xs',
            daemonHealthy
              ? 'text-success cursor-default'
              : 'text-danger hover:bg-danger/10 cursor-pointer',
          )}
        >
          <span
            aria-hidden
            className={cn(
              'h-2 w-2 rounded-full',
              daemonHealthy ? 'bg-success' : 'bg-danger animate-pulse',
            )}
          />
          {daemonHealthy ? 'daemon ok' : 'daemon down'}
        </button>
        <button
          type="button"
          onClick={cycleTheme}
          data-testid="theme-toggle"
          data-theme-pref={themePref}
          aria-label={`Theme: ${THEME_LABEL[themePref]} (${effective}) — click to cycle`}
          title={`Theme: ${THEME_LABEL[themePref]} (${effective})`}
          className={cn(
            'inline-flex items-center justify-center',
            'h-7 w-7 rounded-md',
            'text-base',
            'text-muted hover:text-foreground hover:bg-surface-high/60',
            'transition-colors cursor-pointer',
          )}
        >
          {THEME_GLYPH[themePref]}
        </button>
        <button
          type="button"
          onClick={onOpenPalette}
          data-testid="open-command-palette"
          aria-label="Open command palette (Cmd+K)"
          aria-keyshortcuts="Meta+K Control+K"
          className={cn(
            'inline-flex items-center justify-center',
            'h-7 rounded-md px-2',
            'text-xs font-mono',
            'text-muted hover:text-foreground hover:bg-surface-high/60',
            'transition-colors cursor-pointer',
          )}
        >
          ⌘K
        </button>
      </div>
    </AppShell.Topbar>
  );
}
