import { useState, useEffect, useCallback, useReducer, useRef, useMemo } from 'react';
import { useNavigate, useLocation } from 'react-router-dom';
import type {
  CollaborationPattern,
  Project,
  Task,
  SdiEvent,
} from './types/entities';
import { getJson, listActivePatterns } from './lib/api';
import { daemonUrl } from './lib/daemonUrl';
import { initTheme } from './lib/theme';
import { AppShell } from './components/shell/AppShell';
import { Topbar, type ViewId } from './components/shell/Topbar';
import Sidebar, { type SelectedItem } from './components/Sidebar';
import { SummaryView } from './views/SummaryView';
import { NextView } from './views/NextView';
import { OracleView } from './views/OracleView';
import { QuestionsView } from './views/QuestionsView';
import { BoardView } from './views/BoardView';
import { BacklogView } from './views/BacklogView';
import { TimelineView } from './views/TimelineView';
import { WikiView } from './views/WikiView';
import { PatternsView } from './views/PatternsView';
import { PlanDetail } from './components/PlanDetail';
import { TaskDetail } from './components/TaskDetail';
import { ScenarioDetail } from './components/ScenarioDetail';
import { RoundDetail } from './components/RoundDetail';
import { RequirementDetail } from './components/RequirementDetail';
import { DecisionDetail } from './components/DecisionDetail';
import { ProjectCreateModal } from './components/ProjectCreateModal';
import { ProjectSettingsModal } from './components/ProjectSettingsModal';
import { CreatePlanModal } from './components/CreatePlanModal';
import { CommandPalette } from './components/CommandPalette';
import { ToastContainer } from './components/Toast';
import { cn } from './lib/cn';

const VIEWS = new Set<ViewId>([
  'summary',
  'next',
  'oracle',
  'questions',
  'board',
  'backlog',
  'timeline',
  'wiki',
  'patterns',
]);
const DETAIL_TYPES = new Set([
  'plan',
  'scenario',
  'requirement',
  'decision',
  'round',
  'task',
]);

function parseLocation(pathname: string): {
  projectId: string | null;
  view: ViewId;
  item: SelectedItem | null;
} {
  const parts = pathname.split('/').filter(Boolean);
  let projectId: string | null = null;
  let rest = parts;
  if (parts.length > 0 && parts[0].startsWith('PROJ-')) {
    projectId = parts[0];
    rest = parts.slice(1);
  }
  const view: ViewId = VIEWS.has(rest[0] as ViewId)
    ? (rest[0] as ViewId)
    : 'summary';
  if (rest.length >= 3 && DETAIL_TYPES.has(rest[1])) {
    return {
      projectId,
      view,
      item: { type: rest[1] as SelectedItem['type'], id: rest[2] },
    };
  }
  return { projectId, view, item: null };
}

interface SseState {
  /** Bumped on every event so views can re-fetch. */
  structuralSeq: number;
  /** Task patches keyed by id; null = deleted. */
  taskPatches: Map<string, Task | null>;
  /** Bumped only on task events. */
  patchSeq: number;
}

const SSE_INITIAL: SseState = {
  structuralSeq: 0,
  taskPatches: new Map(),
  patchSeq: 0,
};

type SseAction =
  | { type: 'task:patch'; payload: Task }
  | { type: 'task:delete'; payload: { id: string } }
  | { type: 'structural' };

const DRAWER_WIDTH_KEY = 'sdi.drawer.width';
const DRAWER_DEFAULT_WIDTH = 520;
const DRAWER_MIN_WIDTH = 360;

function clampDrawerWidth(width: number): number {
  // Overlay drawer: up to 90vw (Clawket parity), never below the narrowest
  // readable detail layout.
  const max = Math.max(DRAWER_MIN_WIDTH, window.innerWidth * 0.9);
  return Math.max(DRAWER_MIN_WIDTH, Math.min(max, width));
}

function sseReducer(state: SseState, action: SseAction): SseState {
  switch (action.type) {
    case 'task:patch': {
      const patches = new Map(state.taskPatches);
      patches.set(action.payload.id, action.payload);
      return {
        ...state,
        taskPatches: patches,
        patchSeq: state.patchSeq + 1,
        structuralSeq: state.structuralSeq + 1,
      };
    }
    case 'task:delete': {
      const patches = new Map(state.taskPatches);
      patches.set(action.payload.id, null);
      return {
        ...state,
        taskPatches: patches,
        patchSeq: state.patchSeq + 1,
        structuralSeq: state.structuralSeq + 1,
      };
    }
    case 'structural':
      return { ...state, structuralSeq: state.structuralSeq + 1 };
    default:
      return state;
  }
}

export function App() {
  const navigate = useNavigate();
  const location = useLocation();

  const [projects, setProjects] = useState<Project[]>([]);
  const [selectedProjectId, setSelectedProjectId] = useState<string | null>(null);
  const [sseState, dispatchSse] = useReducer(sseReducer, SSE_INITIAL);
  const [daemonHealthy, setDaemonHealthy] = useState(true);
  const [activePatterns, setActivePatterns] = useState<CollaborationPattern[]>([]);

  // Detail drawer width — drag-resizable from its left edge (parity with the
  // Clawket dashboard). Persisted across sessions.
  const [drawerWidth, setDrawerWidth] = useState(() => {
    const saved = localStorage.getItem(DRAWER_WIDTH_KEY);
    const parsed = saved ? parseInt(saved, 10) : NaN;
    return Number.isFinite(parsed) ? parsed : DRAWER_DEFAULT_WIDTH;
  });
  const isResizingDrawer = useRef(false);

  useEffect(() => {
    const onPointerMove = (e: PointerEvent) => {
      if (!isResizingDrawer.current) return;
      setDrawerWidth(clampDrawerWidth(window.innerWidth - e.clientX));
    };
    const onPointerUp = () => {
      if (!isResizingDrawer.current) return;
      isResizingDrawer.current = false;
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
      setDrawerWidth((w) => {
        try {
          localStorage.setItem(DRAWER_WIDTH_KEY, String(w));
        } catch {
          /* private mode / quota — ignore */
        }
        return w;
      });
    };
    window.addEventListener('pointermove', onPointerMove);
    window.addEventListener('pointerup', onPointerUp);
    return () => {
      window.removeEventListener('pointermove', onPointerMove);
      window.removeEventListener('pointerup', onPointerUp);
    };
  }, []);

  const startDrawerResize = useCallback(() => {
    isResizingDrawer.current = true;
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
  }, []);

  const [projectCreateOpen, setProjectCreateOpen] = useState(false);
  /** Project id whose settings drawer is open; null = closed. Lifted to
   *  App so the modal sees the freshly-fetched project list (the same
   *  list Sidebar/ProjectSwitcher use), and a delete-cascade can update
   *  every consumer in one revalidation pass. */
  const [settingsProjectId, setSettingsProjectId] = useState<string | null>(null);
  const [planCreateOpen, setPlanCreateOpen] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);

  const { projectId: urlProjectId, view: activeView, item: selectedItem } =
    parseLocation(location.pathname);

  if (urlProjectId && urlProjectId !== selectedProjectId) {
    setSelectedProjectId(urlProjectId);
  }

  const urlPrefix = selectedProjectId ? `/${selectedProjectId}` : '';

  const setActiveView = useCallback(
    (view: ViewId) => {
      navigate(`${urlPrefix}/${view}`);
    },
    [navigate, urlPrefix],
  );

  const setSelectedItem = useCallback(
    (item: SelectedItem | null) => {
      if (!item) navigate(`${urlPrefix}/${activeView}`);
      else navigate(`${urlPrefix}/${activeView}/${item.type}/${item.id}`);
    },
    [navigate, urlPrefix, activeView],
  );

  // Root redirect.
  useEffect(() => {
    if (location.pathname === '/') {
      navigate('/summary', { replace: true });
    }
  }, [location.pathname, navigate]);

  // Re-fetch the project list. Used by ProjectSettingsModal after edits +
  // delete-cascade so every consumer (sidebar switcher, settings drawer,
  // active-project state) lands on the same snapshot.
  const refetchProjects = useCallback(async () => {
    try {
      const list = await getJson<Project[]>('/projects');
      setProjects(list);
      return list;
    } catch (err) {
      console.error('Failed to refetch projects:', err);
      setDaemonHealthy(false);
      return [] as Project[];
    }
  }, []);

  // Initial project list.
  useEffect(() => {
    let cancelled = false;
    getJson<Project[]>('/projects')
      .then((list) => {
        if (cancelled) return;
        setProjects(list);
        if (list.length > 0) {
          setSelectedProjectId((prev) => {
            if (prev) return prev;
            // Push to URL so the next navigation keeps the project.
            navigate(`/${list[0].id}/summary`, { replace: true });
            return list[0].id;
          });
        }
      })
      .catch((err) => {
        console.error('Failed to load projects:', err);
        setDaemonHealthy(false);
      });
    return () => {
      cancelled = true;
    };
  }, [navigate]);

  // Theme init.
  useEffect(() => initTheme(), []);

  // Health check.
  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setInterval> | null = null;
    const probe = async () => {
      try {
        await getJson<{ ok: boolean }>('/health');
        if (!cancelled) setDaemonHealthy(true);
      } catch {
        if (!cancelled) setDaemonHealthy(false);
      }
    };
    probe();
    timer = setInterval(probe, 10000);
    return () => {
      cancelled = true;
      if (timer) clearInterval(timer);
    };
  }, []);

  // SSE.
  const sseRef = useRef<EventSource | null>(null);
  useEffect(() => {
    const es = new EventSource(daemonUrl('/events'), { withCredentials: true });
    sseRef.current = es;
    es.onmessage = (msg) => {
      if (!msg.data) return;
      try {
        const ev = JSON.parse(msg.data) as SdiEvent<unknown>;
        if (ev.kind.startsWith('task.')) {
          if (ev.kind === 'task.deleted') {
            const id = ev.entity_id ?? (ev.payload as Partial<Task>)?.id;
            if (id) dispatchSse({ type: 'task:delete', payload: { id } });
          } else {
            const task = ev.payload as Task;
            if (task && typeof task.id === 'string') {
              dispatchSse({ type: 'task:patch', payload: task });
            } else {
              dispatchSse({ type: 'structural' });
            }
          }
        } else {
          dispatchSse({ type: 'structural' });
        }
      } catch {
        /* malformed envelope — daemon contract violation, ignore */
      }
    };
    es.onerror = () => {
      setDaemonHealthy(false);
    };
    es.onopen = () => setDaemonHealthy(true);
    return () => {
      es.close();
      sseRef.current = null;
    };
  }, []);

  // Active CollaborationPattern badge — re-fetches on every structural SSE
  // and on project switch. Scoped to the selected project so the count agrees
  // with the project's Patterns view instead of summing every project in the
  // database. Daemon emits `pattern_created` / `pattern_lifecycle` /
  // `pattern_aborted` which all collapse to `structural` in our reducer.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const rows = selectedProjectId
          ? await listActivePatterns(selectedProjectId)
          : [];
        if (!cancelled) setActivePatterns(rows);
      } catch {
        // Endpoint may be absent during partial daemon rollouts — fall back
        // to an empty list so the badge renders 0 instead of crashing.
        if (!cancelled) setActivePatterns([]);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [sseState.structuralSeq, selectedProjectId]);

  // `direct` rows are solo-flow markers (D23 anti-pattern badge), permanently
  // lifecycle-active by design — counting them as "active patterns" would
  // inflate the number by one per solo plan forever. The count carries real
  // orchestration; the red dot carries solo-flow presence.
  const activeOrchestrationCount = useMemo(
    () => activePatterns.filter((p) => p.kind !== 'direct').length,
    [activePatterns],
  );
  const hasDirectAnti = useMemo(
    () => activePatterns.some((p) => p.kind === 'direct'),
    [activePatterns],
  );

  // Global cmd-K palette.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        e.preventDefault();
        setPaletteOpen((v) => !v);
      } else if (e.key === 'Escape') {
        setPaletteOpen(false);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const selectProject = useCallback(
    (id: string) => {
      setSelectedProjectId(id);
      navigate(`/${id}/${activeView}`);
    },
    [navigate, activeView],
  );

  // Drawer rendering — render the appropriate detail for the selected item.
  let drawerContent: React.ReactNode = null;
  if (selectedItem) {
    switch (selectedItem.type) {
      case 'plan':
        drawerContent = (
          <PlanDetail
            planId={selectedItem.id}
            onClose={() => setSelectedItem(null)}
            refreshKey={sseState.structuralSeq}
            onSelectDecision={(id) => setSelectedItem({ type: 'decision', id })}
          />
        );
        break;
      case 'scenario':
        drawerContent = <ScenarioDetail scenarioId={selectedItem.id} onClose={() => setSelectedItem(null)} refreshKey={sseState.structuralSeq} />;
        break;
      case 'requirement':
        drawerContent = <RequirementDetail requirementId={selectedItem.id} onClose={() => setSelectedItem(null)} refreshKey={sseState.structuralSeq} />;
        break;
      case 'decision':
        drawerContent = <DecisionDetail decisionId={selectedItem.id} onClose={() => setSelectedItem(null)} refreshKey={sseState.structuralSeq} />;
        break;
      case 'round':
        drawerContent = <RoundDetail roundId={selectedItem.id} onClose={() => setSelectedItem(null)} refreshKey={sseState.structuralSeq} />;
        break;
      case 'task':
        drawerContent = <TaskDetail taskId={selectedItem.id} onClose={() => setSelectedItem(null)} refreshKey={sseState.structuralSeq} />;
        break;
    }
  }

  const mainBody = (() => {
    if (!selectedProjectId) {
      return (
        <div className="flex h-full items-center justify-center text-center">
          <div className="space-y-3">
            <p className="text-muted text-sm">No project selected.</p>
            <button
              type="button"
              onClick={() => setProjectCreateOpen(true)}
              className="rounded-md bg-primary px-4 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary-hover transition-colors cursor-pointer"
            >
              ＋ Create project
            </button>
          </div>
        </div>
      );
    }
    switch (activeView) {
      case 'summary':
        return <SummaryView projectId={selectedProjectId} refreshKey={sseState.structuralSeq} />;
      case 'next':
        return <NextView projectId={selectedProjectId} refreshKey={sseState.structuralSeq} />;
      case 'oracle':
        return <OracleView projectId={selectedProjectId} refreshKey={sseState.structuralSeq} />;
      case 'questions':
        return <QuestionsView projectId={selectedProjectId} refreshKey={sseState.structuralSeq} />;
      case 'board':
        return <BoardView projectId={selectedProjectId} refreshKey={sseState.structuralSeq} taskPatches={sseState.taskPatches} onSelectTask={(id) => setSelectedItem({ type: 'task', id })} />;
      case 'backlog':
        return <BacklogView projectId={selectedProjectId} refreshKey={sseState.structuralSeq} onSelectTask={(id) => setSelectedItem({ type: 'task', id })} />;
      case 'timeline':
        return <TimelineView projectId={selectedProjectId} refreshKey={sseState.structuralSeq} />;
      case 'wiki':
        return <WikiView projectId={selectedProjectId} refreshKey={sseState.structuralSeq} />;
      case 'patterns':
        return (
          <PatternsView
            projectId={selectedProjectId}
            refreshKey={sseState.structuralSeq}
            onSelectDecision={(id) => setSelectedItem({ type: 'decision', id })}
          />
        );
    }
  })();

  return (
    <AppShell.Root>
      <Sidebar
        projects={projects}
        selectedId={selectedProjectId}
        onSelect={selectProject}
        onOpenProjectCreate={() => setProjectCreateOpen(true)}
        onOpenProjectSettings={(id) => setSettingsProjectId(id)}
        refreshKey={sseState.structuralSeq}
        selectedItem={selectedItem}
        onSelectItem={setSelectedItem}
        onCreatePlan={() => setPlanCreateOpen(true)}
      />
      <AppShell.Content>
        <Topbar
          activeView={activeView}
          onViewChange={setActiveView}
          onOpenPalette={() => setPaletteOpen(true)}
          daemonHealthy={daemonHealthy}
          onReconnect={() => window.location.reload()}
          activePatternCount={activeOrchestrationCount}
          hasDirectAnti={hasDirectAnti}
          onOpenPatterns={() => setActiveView('patterns')}
        />
        <AppShell.Main>
          <div className="h-full">{mainBody}</div>
          {/* Detail drawer — an OVERLAY above the main content (parity with
              the Clawket dashboard), never an inline panel that squeezes the
              board. Backdrop click closes; the left edge drag-resizes. */}
          {drawerContent && (
            <>
              <div
                data-testid="drawer-backdrop"
                className="fixed inset-0 z-40 bg-overlay transition-opacity"
                onClick={() => setSelectedItem(null)}
              />
              <aside
                data-testid="detail-drawer"
                className={cn(
                  'fixed top-0 right-0 z-50 h-full max-w-[90vw]',
                  'bg-surface shadow-2xl border-l border-border',
                  'animate-slide-in flex',
                )}
                style={{ width: `${drawerWidth}px` }}
              >
                <div
                  role="separator"
                  aria-orientation="vertical"
                  aria-label="Resize detail panel"
                  data-testid="drawer-resize-handle"
                  onPointerDown={startDrawerResize}
                  className={cn(
                    'w-1 hover:w-1.5 shrink-0 cursor-col-resize',
                    'bg-transparent hover:bg-primary/30 transition-colors',
                    'touch-none',
                  )}
                />
                <div className="min-w-0 flex-1 overflow-auto">{drawerContent}</div>
              </aside>
            </>
          )}
        </AppShell.Main>
      </AppShell.Content>
      {projectCreateOpen && (
        <ProjectCreateModal
          onClose={() => setProjectCreateOpen(false)}
          onCreated={(p) => {
            setProjects((prev) => [...prev, p]);
            setSelectedProjectId(p.id);
            navigate(`/${p.id}/summary`);
            setProjectCreateOpen(false);
          }}
        />
      )}
      {settingsProjectId &&
        (() => {
          const project = projects.find((p) => p.id === settingsProjectId);
          if (!project) {
            // Project was deleted (or list hasn't loaded yet); fall through.
            return null;
          }
          return (
            <ProjectSettingsModal
              project={project}
              onClose={() => setSettingsProjectId(null)}
              onProjectChange={refetchProjects}
              onDeleted={async () => {
                const list = await refetchProjects();
                setSettingsProjectId(null);
                if (selectedProjectId === project.id) {
                  if (list.length > 0) {
                    setSelectedProjectId(list[0].id);
                    navigate(`/${list[0].id}/summary`, { replace: true });
                  } else {
                    setSelectedProjectId(null);
                    navigate('/summary', { replace: true });
                  }
                }
              }}
            />
          );
        })()}
      {planCreateOpen && selectedProjectId && (
        <CreatePlanModal
          projectId={selectedProjectId}
          onClose={() => setPlanCreateOpen(false)}
          onCreated={(plan) => {
            setPlanCreateOpen(false);
            setSelectedItem({ type: 'plan', id: plan.id });
          }}
        />
      )}
      {paletteOpen && (
        <CommandPalette
          onClose={() => setPaletteOpen(false)}
          commands={[
            {
              id: 'view-summary',
              label: 'Go to Summary',
              keywords: 'summary',
              run: () => {
                setActiveView('summary');
                setPaletteOpen(false);
              },
            },
            {
              id: 'view-oracle',
              label: 'Go to Oracle',
              keywords: 'oracle completeness verify coverage ssot',
              run: () => {
                setActiveView('oracle');
                setPaletteOpen(false);
              },
            },
            {
              id: 'view-questions',
              label: 'Go to Questions',
              keywords: 'questions decision answer fact preference',
              run: () => {
                setActiveView('questions');
                setPaletteOpen(false);
              },
            },
            {
              id: 'view-board',
              label: 'Go to Board',
              keywords: 'board kanban',
              run: () => {
                setActiveView('board');
                setPaletteOpen(false);
              },
            },
            {
              id: 'view-backlog',
              label: 'Go to Backlog',
              keywords: 'backlog tasks',
              run: () => {
                setActiveView('backlog');
                setPaletteOpen(false);
              },
            },
            {
              id: 'view-timeline',
              label: 'Go to Timeline',
              keywords: 'timeline activity',
              run: () => {
                setActiveView('timeline');
                setPaletteOpen(false);
              },
            },
            {
              id: 'view-wiki',
              label: 'Go to Wiki',
              keywords: 'wiki knowledge docs',
              run: () => {
                setActiveView('wiki');
                setPaletteOpen(false);
              },
            },
            {
              id: 'view-patterns',
              label: 'Go to Patterns',
              keywords: 'patterns collaboration reversibility claims rollback',
              run: () => {
                setActiveView('patterns');
                setPaletteOpen(false);
              },
            },
            {
              id: 'create-plan',
              label: '＋ Create plan…',
              keywords: 'new plan create',
              run: () => {
                setPlanCreateOpen(true);
                setPaletteOpen(false);
              },
            },
            {
              id: 'create-project',
              label: '＋ Create project…',
              keywords: 'new project create',
              run: () => {
                setProjectCreateOpen(true);
                setPaletteOpen(false);
              },
            },
          ]}
        />
      )}
      <ToastContainer />
    </AppShell.Root>
  );
}
