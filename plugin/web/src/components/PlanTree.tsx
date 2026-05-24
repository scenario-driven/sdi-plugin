import { useEffect, useState } from 'react';
import type {
  Plan,
  Scenario,
  Requirement,
  Decision,
  Round,
  Task,
} from '../types/entities';
import { getJson } from '../lib/api';
import { cn } from '../lib/cn';
import type { SelectedItem } from './Sidebar';

interface PlanBundle {
  plan: Plan;
  scenarios: Scenario[];
  requirements: Requirement[];
  decisions: Decision[];
  rounds: Round[];
  /** Tasks bucketed by round_id for fast lookup. */
  tasksByRound: Map<string, Task[]>;
}

interface PlanTreeProps {
  projectId: string;
  selectedItem: SelectedItem | null;
  onSelectItem: (item: SelectedItem) => void;
  onCreatePlan: () => void;
}

const PLAN_STATUS_ACCENT: Record<Plan['status'], string> = {
  draft: 'border-l-2 border-l-border',
  active: 'border-l-2 border-l-primary',
  completed: 'border-l-2 border-l-success/40',
};

const PLAN_STATUS_TINT: Record<Plan['status'], string> = {
  draft: '',
  active: '',
  completed: 'opacity-70',
};

const taskStatusIcon: Record<Task['status'], { icon: string; color: string }> = {
  todo: { icon: '○', color: 'text-muted' },
  in_progress: { icon: '◐', color: 'text-warning' },
  done: { icon: '●', color: 'text-success' },
  blocked: { icon: '⊘', color: 'text-danger' },
  cancelled: { icon: '✕', color: 'text-muted' },
};

const SCENARIO_STATUS_GLYPH: Record<Scenario['status'], string> = {
  proposed: '◇',
  confirmed: '◆',
};

const SCENARIO_STATUS_COLOR: Record<Scenario['status'], string> = {
  proposed: 'text-muted',
  confirmed: 'text-success',
};

const ROUND_STATUS_COLOR: Record<Round['status'], string> = {
  planning: 'text-muted',
  active: 'text-warning',
  completed: 'text-success',
};

export function PlanTree({ projectId, selectedItem, onSelectItem, onCreatePlan }: PlanTreeProps) {
  const [bundles, setBundles] = useState<PlanBundle[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [expandedPlans, setExpandedPlans] = useState<Set<string>>(new Set());
  const [expandedSections, setExpandedSections] = useState<Set<string>>(new Set());

  useEffect(() => {
    let cancelled = false;
    (async () => {
      setLoading(true);
      setError(null);
      try {
        const plans = await getJson<Plan[]>(`/plans?project_id=${projectId}`);
        const result: PlanBundle[] = await Promise.all(
          plans.map(async (plan) => {
            const [scenarios, requirements, decisions, rounds] = await Promise.all([
              getJson<Scenario[]>(`/scenarios?plan_id=${plan.id}`).catch(() => []),
              getJson<Requirement[]>(`/requirements?plan_id=${plan.id}`).catch(() => []),
              getJson<Decision[]>(`/decisions?plan_id=${plan.id}`).catch(() => []),
              getJson<Round[]>(`/rounds?plan_id=${plan.id}`).catch(() => []),
            ]);
            const tasksByRound = new Map<string, Task[]>();
            await Promise.all(
              rounds.map(async (r) => {
                const tasks = await getJson<Task[]>(`/tasks?round_id=${r.id}`).catch(() => []);
                tasksByRound.set(r.id, tasks);
              }),
            );
            return { plan, scenarios, requirements, decisions, rounds, tasksByRound };
          }),
        );
        if (cancelled) return;
        setBundles(result);
        // Auto-expand the active plan on first load.
        const active = result.find((b) => b.plan.status === 'active');
        if (active) {
          setExpandedPlans((prev) => {
            const next = new Set(prev);
            next.add(active.plan.id);
            return next;
          });
        }
      } catch (err) {
        if (!cancelled) {
          const msg = err instanceof Error ? err.message : String(err);
          setError(msg);
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [projectId]);

  function togglePlan(id: string) {
    setExpandedPlans((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  function toggleSection(key: string) {
    setExpandedSections((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }

  if (loading && bundles.length === 0) {
    return <div className="px-4 py-6 text-center text-muted text-xs">Loading…</div>;
  }
  if (error) {
    return <div className="px-4 py-6 text-center text-danger text-xs">{error}</div>;
  }

  return (
    <div className="py-2">
      <div className="px-3 mb-2 flex items-center justify-between">
        <span className="text-xs uppercase tracking-wide text-muted">Plans</span>
        <button
          type="button"
          onClick={onCreatePlan}
          aria-label="Create plan"
          title="Create plan"
          className="text-xs text-muted hover:text-foreground transition-colors cursor-pointer px-1"
        >
          ＋
        </button>
      </div>
      {bundles.length === 0 ? (
        <div className="px-4 py-6 text-center text-muted text-xs">
          No plans yet
        </div>
      ) : (
        <ul className="space-y-0.5">
          {bundles.map((b) => (
            <PlanNode
              key={b.plan.id}
              bundle={b}
              expanded={expandedPlans.has(b.plan.id)}
              onToggle={() => togglePlan(b.plan.id)}
              expandedSections={expandedSections}
              onToggleSection={toggleSection}
              selectedItem={selectedItem}
              onSelectItem={onSelectItem}
            />
          ))}
        </ul>
      )}
    </div>
  );
}

interface PlanNodeProps {
  bundle: PlanBundle;
  expanded: boolean;
  onToggle: () => void;
  expandedSections: Set<string>;
  onToggleSection: (key: string) => void;
  selectedItem: SelectedItem | null;
  onSelectItem: (item: SelectedItem) => void;
}

function PlanNode({
  bundle,
  expanded,
  onToggle,
  expandedSections,
  onToggleSection,
  selectedItem,
  onSelectItem,
}: PlanNodeProps) {
  const { plan, scenarios, requirements, decisions, rounds, tasksByRound } = bundle;
  const isSelected = selectedItem?.type === 'plan' && selectedItem.id === plan.id;
  return (
    <li>
      <div
        className={cn(
          'flex items-center gap-1 px-2 py-1 cursor-pointer',
          'hover:bg-surface-high',
          PLAN_STATUS_ACCENT[plan.status],
          PLAN_STATUS_TINT[plan.status],
          isSelected && 'bg-surface-high',
        )}
        onClick={() => onSelectItem({ type: 'plan', id: plan.id })}
      >
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            onToggle();
          }}
          aria-label={expanded ? 'Collapse plan' : 'Expand plan'}
          className="shrink-0 w-4 text-muted text-xs"
        >
          {expanded ? '▾' : '▸'}
        </button>
        <span className="shrink-0 text-xs font-mono text-muted">{plan.short_code}</span>
        <span className="min-w-0 flex-1 truncate text-sm text-foreground" title={plan.title}>
          {plan.title}
        </span>
        <span
          className={cn(
            'shrink-0 text-[10px] uppercase tracking-wide px-1.5 py-0.5 rounded',
            plan.status === 'active' && 'bg-primary/15 text-primary',
            plan.status === 'draft' && 'bg-muted/15 text-muted',
            plan.status === 'completed' && 'bg-success/15 text-success',
          )}
        >
          {plan.status}
        </span>
      </div>
      {expanded && (
        <div className="ml-4 mt-0.5 mb-1 border-l border-border pl-2 space-y-0.5">
          <Section
            label="Scenarios"
            count={scenarios.length}
            sectionKey={`${plan.id}-scenarios`}
            expanded={expandedSections.has(`${plan.id}-scenarios`)}
            onToggle={onToggleSection}
          >
            {scenarios.map((s) => {
              const sel = selectedItem?.type === 'scenario' && selectedItem.id === s.id;
              return (
                <button
                  key={s.id}
                  type="button"
                  onClick={() => onSelectItem({ type: 'scenario', id: s.id })}
                  className={cn(
                    'flex w-full items-center gap-1 px-2 py-0.5 text-left cursor-pointer hover:bg-surface-high rounded',
                    sel && 'bg-surface-high',
                  )}
                  title={`${s.given} / ${s.when} / ${s.then}`}
                >
                  <span className={cn('w-3 text-center', SCENARIO_STATUS_COLOR[s.status])}>
                    {SCENARIO_STATUS_GLYPH[s.status]}
                  </span>
                  <span className="shrink-0 text-[10px] font-mono text-muted">
                    {s.short_code}
                  </span>
                  <span className="min-w-0 flex-1 truncate text-xs text-foreground">
                    {s.when}
                  </span>
                </button>
              );
            })}
          </Section>
          <Section
            label="Requirements"
            count={requirements.length}
            sectionKey={`${plan.id}-requirements`}
            expanded={expandedSections.has(`${plan.id}-requirements`)}
            onToggle={onToggleSection}
          >
            {requirements.map((r) => {
              const sel = selectedItem?.type === 'requirement' && selectedItem.id === r.id;
              return (
                <button
                  key={r.id}
                  type="button"
                  onClick={() => onSelectItem({ type: 'requirement', id: r.id })}
                  className={cn(
                    'flex w-full items-center gap-1 px-2 py-0.5 text-left cursor-pointer hover:bg-surface-high rounded',
                    sel && 'bg-surface-high',
                  )}
                >
                  <span className="w-3 text-center text-muted text-xs">§</span>
                  <span className="shrink-0 text-[10px] font-mono text-muted">
                    {r.short_code}
                  </span>
                  <span className="min-w-0 flex-1 truncate text-xs text-foreground">
                    {r.body}
                  </span>
                </button>
              );
            })}
          </Section>
          <Section
            label="Decisions"
            count={decisions.length}
            sectionKey={`${plan.id}-decisions`}
            expanded={expandedSections.has(`${plan.id}-decisions`)}
            onToggle={onToggleSection}
          >
            {decisions.map((d) => {
              const sel = selectedItem?.type === 'decision' && selectedItem.id === d.id;
              return (
                <button
                  key={d.id}
                  type="button"
                  onClick={() => onSelectItem({ type: 'decision', id: d.id })}
                  className={cn(
                    'flex w-full items-center gap-1 px-2 py-0.5 text-left cursor-pointer hover:bg-surface-high rounded',
                    sel && 'bg-surface-high',
                    d.status === 'superseded' && 'line-through opacity-60',
                  )}
                >
                  <span className="w-3 text-center text-muted text-xs">⚖</span>
                  <span className="shrink-0 text-[10px] font-mono text-muted">
                    {d.short_code}
                  </span>
                  <span className="min-w-0 flex-1 truncate text-xs text-foreground">
                    {d.title}
                  </span>
                </button>
              );
            })}
          </Section>
          <Section
            label="Rounds"
            count={rounds.length}
            sectionKey={`${plan.id}-rounds`}
            expanded={expandedSections.has(`${plan.id}-rounds`)}
            onToggle={onToggleSection}
          >
            {rounds.map((rd) => {
              const tasks = tasksByRound.get(rd.id) ?? [];
              const sel = selectedItem?.type === 'round' && selectedItem.id === rd.id;
              return (
                <div key={rd.id}>
                  <button
                    type="button"
                    onClick={() => onSelectItem({ type: 'round', id: rd.id })}
                    className={cn(
                      'flex w-full items-center gap-1 px-2 py-0.5 text-left cursor-pointer hover:bg-surface-high rounded',
                      sel && 'bg-surface-high',
                    )}
                  >
                    <span className={cn('w-3 text-center text-xs', ROUND_STATUS_COLOR[rd.status])}>
                      {rd.status === 'active' ? '▶' : rd.status === 'completed' ? '✓' : '◌'}
                    </span>
                    <span className="shrink-0 text-[10px] font-mono text-muted">
                      {rd.short_code}
                    </span>
                    <span className="min-w-0 flex-1 truncate text-xs text-foreground">
                      {rd.mode}
                    </span>
                    <span className="shrink-0 text-[10px] text-muted">
                      {tasks.length}
                    </span>
                  </button>
                  {tasks.length > 0 && (
                    <div className="ml-4 border-l border-border pl-2 space-y-0.5">
                      {tasks.map((t) => {
                        const taskSel = selectedItem?.type === 'task' && selectedItem.id === t.id;
                        const icon = taskStatusIcon[t.status];
                        return (
                          <button
                            key={t.id}
                            type="button"
                            onClick={() => onSelectItem({ type: 'task', id: t.id })}
                            className={cn(
                              'flex w-full items-center gap-1 px-2 py-0.5 text-left cursor-pointer hover:bg-surface-high rounded',
                              taskSel && 'bg-surface-high',
                            )}
                          >
                            <span className={cn('w-3 text-center', icon.color)}>
                              {icon.icon}
                            </span>
                            <span className="shrink-0 text-[10px] font-mono text-muted">
                              {t.short_code}
                            </span>
                            <span className="min-w-0 flex-1 truncate text-xs text-foreground">
                              {t.description}
                            </span>
                          </button>
                        );
                      })}
                    </div>
                  )}
                </div>
              );
            })}
          </Section>
        </div>
      )}
    </li>
  );
}

interface SectionProps {
  label: string;
  count: number;
  sectionKey: string;
  expanded: boolean;
  onToggle: (key: string) => void;
  children?: React.ReactNode;
}

function Section({ label, count, sectionKey, expanded, onToggle, children }: SectionProps) {
  return (
    <div>
      <button
        type="button"
        onClick={() => onToggle(sectionKey)}
        className="w-full flex items-center gap-1 px-2 py-0.5 text-left cursor-pointer hover:bg-surface-high rounded"
      >
        <span className="shrink-0 w-3 text-muted text-xs">
          {expanded ? '▾' : '▸'}
        </span>
        <span className="text-[10px] uppercase tracking-wide text-muted">
          {label}
        </span>
        <span className="text-[10px] text-muted">({count})</span>
      </button>
      {expanded && count > 0 && (
        <div className="ml-3 mt-0.5 space-y-0.5">{children}</div>
      )}
    </div>
  );
}
