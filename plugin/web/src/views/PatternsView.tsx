import { useEffect, useState } from 'react';
import type { Plan } from '../types/entities';
import { getJson } from '../lib/api';
import { PatternTimeline } from '../components/PatternTimeline';
import { PatternTree } from '../components/PatternTree';
import { ReversibilityView } from '../components/ReversibilityView';
import { ClaimsLedger } from '../components/ClaimsLedger';
import { cn } from '../lib/cn';

type TabId = 'timeline' | 'tree' | 'reversibility' | 'claims';

interface PatternsViewProps {
  projectId: string;
  refreshKey: number;
  onSelectDecision?: (decisionId: string) => void;
}

const TABS: ReadonlyArray<{ id: TabId; label: string }> = [
  { id: 'timeline', label: 'Timeline' },
  { id: 'tree', label: 'Tree' },
  { id: 'reversibility', label: 'Reversibility' },
  { id: 'claims', label: 'Claims' },
];

export function PatternsView({ projectId, refreshKey, onSelectDecision }: PatternsViewProps) {
  const [plans, setPlans] = useState<Plan[]>([]);
  const [activePlanId, setActivePlanId] = useState<string | null>(null);
  const [tab, setTab] = useState<TabId>('timeline');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const list = await getJson<Plan[]>(`/plans?project_id=${projectId}`);
        if (cancelled) return;
        setPlans(list);
        setActivePlanId((prev) => {
          if (prev && list.some((p) => p.id === prev)) return prev;
          return (
            list.find((p) => p.status === 'active')?.id ??
            list[0]?.id ??
            null
          );
        });
      } catch (err) {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [projectId, refreshKey]);

  return (
    <div className="h-full flex flex-col">
      <header className="px-6 pt-5 pb-3 border-b border-border space-y-3">
        <div>
          <h2 className="text-headline-lg text-foreground">Patterns</h2>
          <p className="text-sm text-muted">
            CollaborationPattern lifecycle, reversibility gate, and active
            resource claims for the selected plan.
          </p>
        </div>
        <div className="flex items-center gap-3 flex-wrap">
          <label className="flex items-center gap-2 text-xs text-muted">
            Plan
            <select
              value={activePlanId ?? ''}
              onChange={(e) => setActivePlanId(e.target.value || null)}
              className={cn(
                'rounded border border-border bg-surface px-2 py-1',
                'text-xs text-foreground',
              )}
              disabled={plans.length === 0}
            >
              {plans.length === 0 ? (
                <option value="">(no plans)</option>
              ) : (
                plans.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.short_code} · {p.title}
                  </option>
                ))
              )}
            </select>
          </label>
          <nav
            role="tablist"
            aria-label="Patterns tabs"
            className="flex items-center gap-1 ml-auto"
          >
            {TABS.map((t) => {
              const isActive = t.id === tab;
              return (
                <button
                  key={t.id}
                  type="button"
                  role="tab"
                  aria-selected={isActive}
                  data-tab={t.id}
                  data-active={isActive || undefined}
                  onClick={() => setTab(t.id)}
                  className={cn(
                    'rounded-md px-3 py-1 text-xs font-medium',
                    'transition-colors cursor-pointer',
                    isActive
                      ? 'bg-surface-high text-foreground'
                      : 'text-muted hover:text-foreground hover:bg-surface-high/60',
                  )}
                >
                  {t.label}
                </button>
              );
            })}
          </nav>
        </div>
      </header>

      <div className="flex-1 overflow-auto p-6">
        {error && <p className="text-sm text-danger mb-3">{error}</p>}
        {!activePlanId ? (
          <p className="text-sm text-muted text-center py-12">
            Select a plan to see its patterns.
          </p>
        ) : tab === 'timeline' ? (
          <PatternTimeline planId={activePlanId} refreshKey={refreshKey} />
        ) : tab === 'tree' ? (
          <PatternTree planId={activePlanId} refreshKey={refreshKey} />
        ) : tab === 'reversibility' ? (
          <ReversibilityView
            planId={activePlanId}
            refreshKey={refreshKey}
            onSelectDecision={onSelectDecision}
          />
        ) : (
          <ClaimsLedger planId={activePlanId} refreshKey={refreshKey} />
        )}
      </div>
    </div>
  );
}
