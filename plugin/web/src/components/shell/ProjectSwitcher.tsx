import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { Project } from '../../types/entities';
import { cn } from '../../lib/cn';

interface ProjectSwitcherProps {
  projects: Project[];
  activeProjectId: string | null;
  onSelect: (id: string) => void;
  fallbackLabel?: string;
  /** Click handler for "+ New project" footer item. Sidebar owns the create flow. */
  onCreateProject?: () => void;
}

function projectLabel(p: Project): string {
  return p.name;
}

function projectMatchesQuery(p: Project, q: string): boolean {
  if (!q) return true;
  const needle = q.toLowerCase();
  if (p.name.toLowerCase().includes(needle)) return true;
  if (p.key.toLowerCase().includes(needle)) return true;
  if (p.description && p.description.toLowerCase().includes(needle)) return true;
  if (p.cwds.some((c) => c.toLowerCase().includes(needle))) return true;
  return false;
}

export function ProjectSwitcher({
  projects,
  activeProjectId,
  onSelect,
  fallbackLabel = 'Loading…',
  onCreateProject,
}: ProjectSwitcherProps) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const containerRef = useRef<HTMLDivElement | null>(null);
  const searchRef = useRef<HTMLInputElement | null>(null);

  const active =
    (activeProjectId && projects.find((p) => p.id === activeProjectId)) || null;

  const sorted = useMemo(() => {
    const filtered = projects.filter((p) => projectMatchesQuery(p, query));
    return filtered.sort((a, b) => {
      if (a.id === activeProjectId) return -1;
      if (b.id === activeProjectId) return 1;
      return a.name.localeCompare(b.name);
    });
  }, [projects, query, activeProjectId]);

  const closeMenu = useCallback(() => {
    setOpen(false);
    setQuery('');
  }, []);

  useEffect(() => {
    if (!open) return;
    function onPointerDown(e: MouseEvent) {
      if (!containerRef.current) return;
      if (containerRef.current.contains(e.target as Node)) return;
      closeMenu();
    }
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Escape') closeMenu();
    }
    document.addEventListener('mousedown', onPointerDown);
    document.addEventListener('keydown', onKey);
    const handle = window.setTimeout(() => searchRef.current?.focus(), 0);
    return () => {
      document.removeEventListener('mousedown', onPointerDown);
      document.removeEventListener('keydown', onKey);
      window.clearTimeout(handle);
    };
  }, [open, closeMenu]);

  const buttonLabel = active ? projectLabel(active) : fallbackLabel;
  const disabled = projects.length === 0 && !onCreateProject;

  return (
    <div ref={containerRef} className="relative min-w-0 flex-1">
      <button
        type="button"
        data-testid="project-switcher-button"
        aria-haspopup="listbox"
        aria-expanded={open}
        disabled={disabled}
        onClick={() => setOpen((v) => !v)}
        className={cn(
          'flex w-full items-center gap-1.5',
          'rounded-md px-2 py-1',
          'text-left text-sm font-semibold text-foreground',
          'hover:bg-surface-high focus:bg-surface-high focus:outline-none',
          'disabled:cursor-not-allowed disabled:opacity-60',
          'cursor-pointer',
        )}
      >
        <span className="min-w-0 flex-1 truncate">{buttonLabel}</span>
        <span aria-hidden className="text-xs text-muted">▾</span>
      </button>
      {open && (
        <div
          data-testid="project-switcher-list"
          className={cn(
            'absolute left-0 right-0 top-full z-40 mt-1',
            'rounded-md border border-border bg-surface-high',
            'shadow-xl',
          )}
        >
          <div className="p-2 border-b border-border">
            <input
              ref={searchRef}
              type="text"
              data-testid="project-switcher-search"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search projects…"
              className={cn(
                'w-full rounded-md border border-border bg-surface px-2 py-1',
                'text-sm text-foreground placeholder:text-muted',
                'focus:outline-none focus:border-primary',
              )}
            />
          </div>
          <ul
            role="listbox"
            aria-label="Projects"
            className="max-h-64 overflow-auto py-1"
          >
            {sorted.length === 0 ? (
              <li className="px-3 py-2 text-sm text-muted italic">
                {projects.length === 0 ? 'No projects yet' : 'No matches'}
              </li>
            ) : (
              sorted.map((p) => {
                const isActive = p.id === activeProjectId;
                return (
                  <li key={p.id}>
                    <button
                      type="button"
                      data-testid={`project-switcher-option-${p.id}`}
                      role="option"
                      aria-selected={isActive}
                      onClick={() => {
                        onSelect(p.id);
                        closeMenu();
                      }}
                      className={cn(
                        'flex w-full items-center gap-2 px-3 py-1.5',
                        'text-left text-sm text-foreground',
                        'hover:bg-surface focus:bg-surface focus:outline-none cursor-pointer',
                        isActive && 'bg-surface',
                      )}
                    >
                      <span className="min-w-0 flex-1 truncate">{projectLabel(p)}</span>
                      <span
                        aria-hidden
                        className="text-xs text-muted font-mono"
                      >
                        {p.key}
                      </span>
                      {isActive && (
                        <span aria-hidden className="text-xs text-muted">✓</span>
                      )}
                    </button>
                  </li>
                );
              })
            )}
            {onCreateProject && (
              <li className="mt-1 border-t border-border pt-1">
                <button
                  type="button"
                  data-testid="project-switcher-new"
                  onClick={() => {
                    closeMenu();
                    onCreateProject();
                  }}
                  className={cn(
                    'flex w-full items-center gap-2 px-3 py-1.5',
                    'text-left text-sm text-primary',
                    'hover:bg-surface focus:bg-surface focus:outline-none cursor-pointer',
                  )}
                >
                  <span aria-hidden>＋</span>
                  <span>New project</span>
                </button>
              </li>
            )}
          </ul>
        </div>
      )}
    </div>
  );
}
