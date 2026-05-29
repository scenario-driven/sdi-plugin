import { useEffect, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import type { Plan, Scenario, Requirement, Decision, Round } from '../types/entities';
import { getJson, postJson, DaemonError } from '../lib/api';
import { toastError, toastSuccess } from '../lib/toast';
import { Badge, Button } from './ui';
import { cn } from '../lib/cn';
import { AutonomyPanel } from './AutonomyPanel';
import { DecisionTimeline } from './DecisionTimeline';
import { AgentNotesPanel } from './AgentNotesPanel';

interface PlanDetailProps {
  planId: string;
  onClose: () => void;
  refreshKey: number;
  onSelectDecision?: (id: string) => void;
}

interface PlanBundle {
  plan: Plan;
  scenarios: Scenario[];
  requirements: Requirement[];
  decisions: Decision[];
  rounds: Round[];
}

const STATUS_VARIANT: Record<Plan['status'], 'default' | 'primary' | 'success'> = {
  draft: 'default',
  active: 'primary',
  completed: 'success',
};

export function PlanDetail({ planId, onClose, refreshKey, onSelectDecision }: PlanDetailProps) {
  const [bundle, setBundle] = useState<PlanBundle | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const plan = await getJson<Plan>(`/plans/${planId}`);
        const [scenarios, requirements, decisions, rounds] = await Promise.all([
          getJson<Scenario[]>(`/scenarios?plan_id=${planId}`).catch(() => []),
          getJson<Requirement[]>(`/requirements?plan_id=${planId}`).catch(() => []),
          getJson<Decision[]>(`/decisions?plan_id=${planId}`).catch(() => []),
          getJson<Round[]>(`/rounds?plan_id=${planId}`).catch(() => []),
        ]);
        if (!cancelled) setBundle({ plan, scenarios, requirements, decisions, rounds });
      } catch (err) {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [planId, refreshKey]);

  async function approve() {
    if (!bundle) return;
    setBusy(true);
    try {
      await postJson(`/plans/${bundle.plan.id}/approve`, {});
      toastSuccess(`${bundle.plan.short_code} approved`);
    } catch (err) {
      if (err instanceof DaemonError) toastError(`${err.code}: ${err.message}`);
      else toastError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function complete() {
    if (!bundle) return;
    setBusy(true);
    try {
      await postJson(`/plans/${bundle.plan.id}/complete`, {});
      toastSuccess(`${bundle.plan.short_code} completed`);
    } catch (err) {
      if (err instanceof DaemonError) toastError(`${err.code}: ${err.message}`);
      else toastError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function createRound() {
    if (!bundle) return;
    setBusy(true);
    try {
      await postJson<Round>('/rounds', {
        plan_id: bundle.plan.id,
        mode: 'strict-regression',
        in_flight_policy: 'pause',
        disruption_policy: 'needs-review',
      });
      toastSuccess('Round created');
    } catch (err) {
      if (err instanceof DaemonError) toastError(`${err.code}: ${err.message}`);
      else toastError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function confirmScenario(scenarioId: string) {
    setBusy(true);
    try {
      await postJson(`/scenarios/${scenarioId}/confirm`, {});
      toastSuccess('Scenario confirmed');
    } catch (err) {
      if (err instanceof DaemonError) toastError(`${err.code}: ${err.message}`);
      else toastError(String(err));
    } finally {
      setBusy(false);
    }
  }

  if (error) return <Wrapper onClose={onClose}><p className="text-danger text-sm">{error}</p></Wrapper>;
  if (!bundle) return <Wrapper onClose={onClose}><p className="text-sm text-muted">Loading…</p></Wrapper>;

  const { plan, scenarios, requirements, decisions, rounds } = bundle;
  const canApprove = plan.status === 'draft' && scenarios.some((s) => s.status === 'confirmed');

  return (
    <Wrapper onClose={onClose}>
      <header className="space-y-2">
        <div className="flex items-center gap-2">
          <span className="text-xs font-mono text-muted">{plan.short_code}</span>
          <Badge variant={STATUS_VARIANT[plan.status]} size="sm">
            {plan.status}
          </Badge>
        </div>
        <h2 className="text-headline-lg text-foreground">{plan.title}</h2>
      </header>

      <div className="flex flex-wrap gap-2">
        {plan.status === 'draft' && (
          <Button size="sm" disabled={busy || !canApprove} onClick={approve} title={!canApprove ? 'At least one confirmed scenario required' : undefined}>
            Approve plan
          </Button>
        )}
        {plan.status === 'active' && (
          <Button size="sm" variant="outline" disabled={busy} onClick={complete}>
            Complete plan
          </Button>
        )}
        {plan.status === 'active' && (
          <Button size="sm" variant="secondary" disabled={busy} onClick={createRound}>
            ＋ Round
          </Button>
        )}
      </div>

      {plan.body && (
        <section>
          <h3 className="text-headline-md text-foreground mb-2">Body</h3>
          <div className="rounded-md border border-border bg-background p-4 text-sm">
            <div className="prose prose-sm max-w-none text-foreground prose-headings:text-foreground prose-strong:text-foreground prose-code:text-foreground prose-a:text-primary">
              <ReactMarkdown remarkPlugins={[remarkGfm]}>{plan.body}</ReactMarkdown>
            </div>
          </div>
        </section>
      )}

      <Section title="Scenarios" count={scenarios.length}>
        {scenarios.length === 0 ? (
          <Empty>No scenarios. Add at least one confirmed scenario before approving.</Empty>
        ) : (
          <ul className="divide-y divide-border rounded-md border border-border bg-background">
            {scenarios.map((s) => (
              <li key={s.id} className="px-3 py-2 space-y-1">
                <div className="flex items-center gap-2">
                  <span className="text-[10px] font-mono text-muted">{s.short_code}</span>
                  <Badge size="sm" variant={s.status === 'confirmed' ? 'success' : 'default'}>
                    {s.status}
                  </Badge>
                  {s.status === 'proposed' && (
                    <button
                      type="button"
                      onClick={() => confirmScenario(s.id)}
                      disabled={busy}
                      className="ml-auto text-[10px] text-primary hover:underline cursor-pointer"
                    >
                      confirm →
                    </button>
                  )}
                </div>
                <GwtBlock label="Given" text={s.given} />
                <GwtBlock label="When" text={s.when} />
                <GwtBlock label="Then" text={s.then} />
              </li>
            ))}
          </ul>
        )}
      </Section>

      <Section title="Requirements" count={requirements.length}>
        {requirements.length === 0 ? (
          <Empty>No requirements.</Empty>
        ) : (
          <ul className="divide-y divide-border rounded-md border border-border bg-background">
            {requirements.map((r) => (
              <li key={r.id} className="px-3 py-2">
                <div className="text-[10px] font-mono text-muted">{r.short_code}</div>
                <div className="text-sm text-foreground whitespace-pre-wrap">{r.body}</div>
              </li>
            ))}
          </ul>
        )}
      </Section>

      <Section title="Decisions (D20 — 4-stage)" count={decisions.length}>
        <DecisionTimeline decisions={decisions} onSelect={(id) => onSelectDecision?.(id)} />
      </Section>

      <AutonomyPanel projectId={plan.project_id} planId={plan.id} refreshKey={refreshKey} />

      <AgentNotesPanel
        projectId={plan.project_id}
        scopeKind="plan"
        anchorId={plan.id}
        refreshKey={refreshKey}
      />

      <Section title="Rounds" count={rounds.length}>
        {rounds.length === 0 ? (
          <Empty>No rounds. {plan.status === 'active' ? 'Create one with the button above.' : 'Approve the plan first.'}</Empty>
        ) : (
          <ul className="divide-y divide-border rounded-md border border-border bg-background">
            {rounds.map((r) => (
              <li key={r.id} className="px-3 py-2 flex items-center gap-2">
                <span className="text-[10px] font-mono text-muted">{r.short_code}</span>
                <span className="text-xs text-muted">· {r.mode}</span>
                <Badge size="sm" variant={r.status === 'active' ? 'primary' : r.status === 'completed' ? 'success' : 'default'} className="ml-auto">
                  {r.status}
                </Badge>
              </li>
            ))}
          </ul>
        )}
      </Section>
    </Wrapper>
  );
}

function Wrapper({ onClose, children }: { onClose: () => void; children: React.ReactNode }) {
  return (
    <div className="h-full flex flex-col">
      <div className="px-5 py-3 border-b border-border flex items-center justify-between sticky top-0 bg-surface z-10">
        <span className="text-[10px] uppercase tracking-wide text-muted">Plan</span>
        <button
          type="button"
          onClick={onClose}
          className="text-muted hover:text-foreground text-xl leading-none cursor-pointer"
          aria-label="Close"
        >
          ×
        </button>
      </div>
      <div className="flex-1 overflow-auto p-5 space-y-5">{children}</div>
    </div>
  );
}

function Section({ title, count, children }: { title: string; count: number; children: React.ReactNode }) {
  return (
    <section>
      <h3 className="text-headline-md text-foreground mb-2 flex items-baseline gap-2">
        {title}
        <span className="text-xs text-muted font-normal">{count}</span>
      </h3>
      {children}
    </section>
  );
}

function Empty({ children }: { children: React.ReactNode }) {
  return <p className="text-xs italic text-muted px-2 py-2">{children}</p>;
}

function GwtBlock({ label, text }: { label: string; text: string }) {
  return (
    <div className="flex gap-2">
      <span className={cn('text-[10px] uppercase tracking-wide text-muted w-12 shrink-0 mt-0.5')}>{label}</span>
      <span className="text-xs text-foreground whitespace-pre-wrap">{text}</span>
    </div>
  );
}
