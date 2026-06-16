import { useEffect, useMemo, useState } from 'react';
import {
  DndContext,
  DragOverlay,
  PointerSensor,
  useSensor,
  useSensors,
  useDraggable,
  useDroppable,
  type DragStartEvent,
  type DragEndEvent,
} from '@dnd-kit/core';
import type { Plan, Round, Task, TaskStatus } from '../types/entities';
import { getJson, postJson, DaemonError } from '../lib/api';
import { cn } from '../lib/cn';
import { toastError } from '../lib/toast';

interface BoardViewProps {
  projectId: string;
  refreshKey: number;
  taskPatches: Map<string, Task | null>;
  onSelectTask: (id: string) => void;
}

const COLUMNS: { key: TaskStatus; label: string; tone: string }[] = [
  { key: 'todo', label: 'Todo', tone: 'border-l-muted' },
  { key: 'in_progress', label: 'In Progress', tone: 'border-l-warning' },
  { key: 'blocked', label: 'Blocked', tone: 'border-l-danger' },
  { key: 'done', label: 'Done', tone: 'border-l-success' },
  { key: 'cancelled', label: 'Cancelled', tone: 'border-l-muted/60' },
];

export function BoardView({ projectId, refreshKey, taskPatches, onSelectTask }: BoardViewProps) {
  const [tasks, setTasks] = useState<Task[]>([]);
  const [activePlan, setActivePlan] = useState<Plan | null>(null);
  const [activeRound, setActiveRound] = useState<Round | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [activeTask, setActiveTask] = useState<Task | null>(null);

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
  );

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const plans = await getJson<Plan[]>(`/plans?project_id=${projectId}`);
        const plan = plans.find((p) => p.status === 'active') ?? null;
        if (!plan) {
          if (!cancelled) {
            setActivePlan(null);
            setActiveRound(null);
            setTasks([]);
          }
          return;
        }
        const rounds = await getJson<Round[]>(`/rounds?plan_id=${plan.id}`);
        const round = rounds.find((r) => r.status === 'active') ?? rounds[0] ?? null;
        const ts = round
          ? await getJson<Task[]>(`/tasks?round_id=${round.id}`)
          : [];
        if (cancelled) return;
        setActivePlan(plan);
        setActiveRound(round);
        setTasks(ts);
      } catch (err) {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [projectId, refreshKey]);

  const patched = useMemo(() => {
    if (taskPatches.size === 0) return tasks;
    const map = new Map(tasks.map((t) => [t.id, t]));
    for (const [id, patch] of taskPatches) {
      if (patch === null) map.delete(id);
      else if (activeRound && patch.round_id === activeRound.id) map.set(id, patch);
    }
    return Array.from(map.values());
  }, [tasks, taskPatches, activeRound]);

  const byStatus = useMemo(() => {
    const m = new Map<TaskStatus, Task[]>();
    for (const c of COLUMNS) m.set(c.key, []);
    for (const t of patched) {
      const arr = m.get(t.status);
      if (arr) arr.push(t);
    }
    return m;
  }, [patched]);

  function handleDragStart(e: DragStartEvent) {
    const id = String(e.active.id);
    const t = patched.find((x) => x.id === id);
    if (t) setActiveTask(t);
  }

  async function handleDragEnd(e: DragEndEvent) {
    setActiveTask(null);
    const targetStatus = e.over?.id as TaskStatus | undefined;
    const taskId = String(e.active.id);
    if (!targetStatus) return;
    const t = patched.find((x) => x.id === taskId);
    if (!t || t.status === targetStatus) return;
    // Optimistic update.
    setTasks((prev) => prev.map((x) => (x.id === t.id ? { ...x, status: targetStatus } : x)));
    try {
      await postJson(`/tasks/${t.id}/status`, { status: targetStatus });
    } catch (err) {
      // Rollback.
      setTasks((prev) => prev.map((x) => (x.id === t.id ? t : x)));
      if (err instanceof DaemonError) {
        toastError(`${err.code}: ${err.message}`);
      } else {
        toastError(String(err));
      }
    }
  }

  if (error) return <div className="p-6 text-danger text-sm">{error}</div>;

  if (!activePlan) {
    return (
      <div className="p-10 text-center text-muted text-sm">
        No active plan in this project. Activate a plan to populate the board.
      </div>
    );
  }
  if (!activeRound) {
    return (
      <div className="p-10 text-center text-muted text-sm">
        Plan <span className="font-mono text-foreground">{activePlan.short_code}</span> has no rounds yet.
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col">
      <header className="px-6 pt-5 pb-3 flex items-baseline gap-3 border-b border-border">
        <h2 className="text-headline-lg text-foreground">Board</h2>
        <span className="text-xs text-muted">
          {activePlan.short_code} · round {activeRound.short_code} · {activeRound.mode}
        </span>
      </header>
      <DndContext sensors={sensors} onDragStart={handleDragStart} onDragEnd={handleDragEnd}>
        <div className="flex-1 overflow-auto p-4">
          <div className="grid grid-cols-5 gap-3 min-w-[1100px]">
            {COLUMNS.map((c) => (
              <DroppableColumn key={c.key} columnKey={c.key} label={c.label} tone={c.tone} count={(byStatus.get(c.key) ?? []).length}>
                {(byStatus.get(c.key) ?? []).map((t) => (
                  <TaskCard key={t.id} task={t} onClick={() => onSelectTask(t.id)} />
                ))}
              </DroppableColumn>
            ))}
          </div>
        </div>
        <DragOverlay>
          {activeTask ? <TaskCardView task={activeTask} ghost /> : null}
        </DragOverlay>
      </DndContext>
    </div>
  );
}

interface ColumnProps {
  columnKey: TaskStatus;
  label: string;
  tone: string;
  count: number;
  children: React.ReactNode;
}

function DroppableColumn({ columnKey, label, tone, count, children }: ColumnProps) {
  const { setNodeRef, isOver } = useDroppable({ id: columnKey });
  return (
    <div
      ref={setNodeRef}
      data-testid={`board-column-${columnKey}`}
      className={cn(
        'rounded-md bg-surface border-l-4',
        tone,
        'border-y border-r border-border',
        'flex flex-col min-h-[200px]',
        isOver && 'ring-2 ring-primary',
      )}
    >
      <div className="px-3 py-2 flex items-center justify-between border-b border-border">
        <span className="text-xs uppercase tracking-wide text-muted">{label}</span>
        <span className="text-xs text-muted">{count}</span>
      </div>
      <div className="flex-1 overflow-y-auto p-2 space-y-2">{children}</div>
    </div>
  );
}

function TaskCard({ task, onClick }: { task: Task; onClick: () => void }) {
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({ id: task.id });
  return (
    <div
      ref={setNodeRef}
      {...attributes}
      {...listeners}
      data-testid={`task-card-${task.id}`}
      onClick={onClick}
      role="button"
      tabIndex={0}
      className={cn(
        'rounded-md border border-border bg-background p-2 cursor-pointer',
        'hover:border-primary/50 transition-colors',
        isDragging && 'opacity-50',
      )}
    >
      <TaskCardView task={task} />
    </div>
  );
}

function TaskCardView({ task, ghost = false }: { task: Task; ghost?: boolean }) {
  return (
    <div className={cn(ghost && 'opacity-90 shadow-elevated rounded-md border border-border bg-background p-2')}>
      <div className="text-[10px] font-mono text-muted">{task.short_code}</div>
      <div className="text-sm text-foreground line-clamp-3 mt-0.5">{task.description}</div>
    </div>
  );
}
