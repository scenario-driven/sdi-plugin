import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from 'react';
import type { Project, Plan, Round } from '../types/entities';
import { getJson } from '../lib/api';
import { AppShell } from './shell/AppShell';
import { BrandMark } from './shell/BrandMark';
import { ProjectSwitcher } from './shell/ProjectSwitcher';
import { cn } from '../lib/cn';
import { PlanTree } from './PlanTree';

export type SelectedItem =
  | { type: 'plan'; id: string }
  | { type: 'scenario'; id: string }
  | { type: 'requirement'; id: string }
  | { type: 'decision'; id: string }
  | { type: 'round'; id: string }
  | { type: 'task'; id: string };

const SIDEBAR_WIDTH_KEY = 'sdi.sidebarWidth';
const SIDEBAR_COLLAPSED_KEY = 'sdi.sidebarCollapsed';
const MIN_WIDTH = 200;
const MAX_WIDTH = 480;
const DEFAULT_WIDTH = 288;
const COLLAPSED_WIDTH = 48;

function clampWidth(n: number): number {
  if (!Number.isFinite(n)) return DEFAULT_WIDTH;
  if (n < MIN_WIDTH) return MIN_WIDTH;
  if (n > MAX_WIDTH) return MAX_WIDTH;
  return Math.round(n);
}

function readStoredWidth(): number {
  try {
    const raw = localStorage.getItem(SIDEBAR_WIDTH_KEY);
    if (!raw) return DEFAULT_WIDTH;
    return clampWidth(Number.parseInt(raw, 10));
  } catch {
    return DEFAULT_WIDTH;
  }
}

function writeStoredWidth(width: number): void {
  try {
    localStorage.setItem(SIDEBAR_WIDTH_KEY, String(width));
  } catch {
    // storage unavailable — ignore
  }
}

function readStoredCollapsed(): boolean {
  try {
    return localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === 'true';
  } catch {
    return false;
  }
}

function writeStoredCollapsed(collapsed: boolean): void {
  try {
    localStorage.setItem(SIDEBAR_COLLAPSED_KEY, String(collapsed));
  } catch {
    // storage unavailable — ignore
  }
}

function pickActivePlan(plans: Plan[]): Plan | null {
  return plans.find((p) => p.status === 'active') ?? plans[0] ?? null;
}

function pickActiveRound(rounds: Round[], plan: Plan | null): Round | null {
  if (!plan) return null;
  return rounds.find((r) => r.plan_id === plan.id && r.status === 'active') ?? null;
}

interface SidebarProps {
  projects: Project[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  onOpenProjectCreate: () => void;
  /** Bumps when SSE structural events fire so the active context refreshes. */
  refreshKey?: number;
  selectedItem: SelectedItem | null;
  onSelectItem: (item: SelectedItem | null) => void;
  onCreatePlan: () => void;
}

export default function Sidebar({
  projects,
  selectedId,
  onSelect,
  onOpenProjectCreate,
  refreshKey = 0,
  selectedItem,
  onSelectItem,
  onCreatePlan,
}: SidebarProps) {
  const [collapsed, setCollapsed] = useState<boolean>(readStoredCollapsed);
  const [width, setWidth] = useState<number>(readStoredWidth);

  const [activeContext, setActiveContext] = useState<{
    projectId: string;
    plan: Plan | null;
    round: Round | null;
  } | null>(null);

  useEffect(() => {
    if (!selectedId) return;
    let cancelled = false;
    (async () => {
      try {
        const plans = await getJson<Plan[]>(`/plans?project_id=${selectedId}`);
        const plan = pickActivePlan(plans);
        const rounds = plan
          ? await getJson<Round[]>(`/rounds?plan_id=${plan.id}`).catch(() => [])
          : [];
        if (cancelled) return;
        setActiveContext({
          projectId: selectedId,
          plan,
          round: pickActiveRound(rounds, plan),
        });
      } catch (err) {
        if (!cancelled) console.error('Sidebar: failed to load active context:', err);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [selectedId, refreshKey]);

  const contextMatches =
    activeContext !== null && activeContext.projectId === selectedId;
  const activePlan = contextMatches ? activeContext.plan : null;
  const activeRound = contextMatches ? activeContext.round : null;

  const dragRef = useRef<{ startX: number; startWidth: number } | null>(null);
  const moveHandlerRef = useRef<((e: PointerEvent) => void) | null>(null);
  const upHandlerRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    return () => {
      if (moveHandlerRef.current) {
        window.removeEventListener('pointermove', moveHandlerRef.current);
        moveHandlerRef.current = null;
      }
      if (upHandlerRef.current) {
        window.removeEventListener('pointerup', upHandlerRef.current);
        upHandlerRef.current = null;
      }
    };
  }, []);

  function onHandlePointerDown(e: ReactPointerEvent<HTMLDivElement>) {
    e.preventDefault();
    dragRef.current = { startX: e.clientX, startWidth: width };

    const onMove = (ev: PointerEvent) => {
      const drag = dragRef.current;
      if (!drag) return;
      setWidth(clampWidth(drag.startWidth + (ev.clientX - drag.startX)));
    };
    const onUp = () => {
      if (!dragRef.current) return;
      dragRef.current = null;
      if (moveHandlerRef.current) {
        window.removeEventListener('pointermove', moveHandlerRef.current);
        moveHandlerRef.current = null;
      }
      if (upHandlerRef.current) {
        window.removeEventListener('pointerup', upHandlerRef.current);
        upHandlerRef.current = null;
      }
      setWidth((w) => {
        writeStoredWidth(w);
        return w;
      });
    };

    moveHandlerRef.current = onMove;
    upHandlerRef.current = onUp;
    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
  }

  const toggleCollapse = useCallback(() => {
    setCollapsed((prev) => {
      const next = !prev;
      writeStoredCollapsed(next);
      return next;
    });
  }, []);

  const activeProject = useMemo(
    () => projects.find((p) => p.id === selectedId) ?? null,
    [projects, selectedId],
  );

  if (collapsed) {
    return (
      <AppShell.Sidebar
        data-testid="app-sidebar"
        data-collapsed="true"
        className="overflow-visible"
        style={{ width: `${COLLAPSED_WIDTH}px` }}
      >
        <div className="h-12 shrink-0 flex items-center justify-center border-b border-border">
          <button
            type="button"
            onClick={toggleCollapse}
            title="Expand sidebar"
            aria-label="Expand sidebar"
            className="p-1 text-muted hover:text-foreground transition-colors cursor-pointer"
          >
            <BrandMark size={20} />
          </button>
        </div>
        <nav
          aria-label="Projects"
          className="flex-1 overflow-y-auto py-2 space-y-1"
        >
          {projects.map((p) => (
            <button
              key={p.id}
              type="button"
              onClick={() => onSelect(p.id)}
              title={p.name}
              className={cn(
                'w-full flex justify-center py-2 transition-colors cursor-pointer',
                selectedId === p.id
                  ? 'text-primary bg-primary/15'
                  : 'text-muted hover:text-foreground hover:bg-surface-hover',
              )}
            >
              <span className="text-xs font-bold">
                {p.name.charAt(0).toUpperCase()}
              </span>
            </button>
          ))}
        </nav>
      </AppShell.Sidebar>
    );
  }

  return (
    <AppShell.Sidebar
      data-testid="app-sidebar"
      data-width={width}
      className="relative overflow-visible"
      style={{ width: `${width}px` }}
    >
      <header
        className={cn(
          'shrink-0',
          'flex flex-col',
          'border-b border-border',
        )}
      >
        <div className="h-12 shrink-0 flex items-center gap-2 px-3">
          <BrandMark size={24} className="shrink-0" />
          <span
            data-testid="sidebar-brand-name"
            className="shrink-0 text-sm font-semibold text-foreground"
          >
            SDI
          </span>
          <span className="flex-1" />
          <button
            type="button"
            onClick={toggleCollapse}
            aria-label="Collapse sidebar"
            title="Collapse sidebar"
            className="shrink-0 text-xs text-muted hover:text-foreground transition-colors cursor-pointer px-1"
          >
            {'◀'}
          </button>
        </div>
        <div className="h-10 shrink-0 flex items-center gap-1 px-3 pb-2">
          <ProjectSwitcher
            projects={projects}
            activeProjectId={selectedId}
            onSelect={onSelect}
            onCreateProject={onOpenProjectCreate}
            fallbackLabel={activeProject ? activeProject.name : 'Select project'}
          />
        </div>
      </header>

      <section
        className="flex flex-col gap-1 border-b border-border px-4 py-3"
        aria-label="Active context"
      >
        <p className="text-xs uppercase tracking-wide text-muted">Active</p>
        {activePlan ? (
          <>
            <p
              data-testid="sidebar-active-plan"
              className="text-sm font-medium text-foreground truncate"
              title={activePlan.title}
            >
              {activePlan.title}
            </p>
            <p
              data-testid="sidebar-active-round"
              className="text-xs text-muted truncate"
              title={activeRound?.short_code ?? ''}
            >
              {activeRound
                ? `Round: ${activeRound.short_code} (${activeRound.mode})`
                : 'No active round'}
            </p>
          </>
        ) : (
          <p
            data-testid="sidebar-active-plan"
            className="text-sm text-muted italic"
          >
            {selectedId ? 'No active plan' : 'Select a project'}
          </p>
        )}
      </section>

      <nav
        aria-label="Plan tree"
        className="min-h-0 flex-1 overflow-auto"
      >
        {selectedId ? (
          <PlanTree
            key={`plantree-${selectedId}-${refreshKey}`}
            projectId={selectedId}
            selectedItem={selectedItem}
            onSelectItem={onSelectItem}
            onCreatePlan={onCreatePlan}
          />
        ) : (
          <div className="px-4 py-6 text-center text-muted text-sm">
            {projects.length === 0 ? 'No projects yet' : 'Select a project'}
          </div>
        )}
      </nav>

      <div
        data-testid="sidebar-resize-handle"
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize sidebar"
        onPointerDown={onHandlePointerDown}
        className={cn(
          'absolute right-0 top-0 h-full w-1.5 translate-x-1/2 z-10',
          'cursor-col-resize',
          'hover:bg-primary/40',
        )}
      />
    </AppShell.Sidebar>
  );
}
