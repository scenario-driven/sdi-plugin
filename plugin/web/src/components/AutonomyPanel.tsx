import { useEffect, useState, useCallback } from 'react';
import type { AutonomyMode, AutonomyPolicy, AutonomyScopeKind } from '../types/entities';
import { getJson, postJson, DaemonError } from '../lib/api';
import { toastError, toastSuccess } from '../lib/toast';
import { Badge, Button, Select, Input } from './ui';
import { cn } from '../lib/cn';

interface AutonomyPanelProps {
  projectId: string;
  planId?: string | null;
  refreshKey: number;
}

interface ListResponse {
  policies: AutonomyPolicy[];
}

interface ResolveResponse {
  policy: AutonomyPolicy | null;
}

const MODE_LABEL: Record<AutonomyMode, string> = {
  L3: 'L3 · ask',
  L4: 'L4 · act with review',
  L5: 'L5 · act and notify',
};

const MODE_VARIANT: Record<AutonomyMode, 'default' | 'primary' | 'success' | 'warning'> = {
  L3: 'warning',
  L4: 'primary',
  L5: 'success',
};

const FORCED_L4_KINDS = new Set(['architecture', 'schema', 'naming-canonical']);

export function AutonomyPanel({ projectId, planId, refreshKey }: AutonomyPanelProps) {
  const [policies, setPolicies] = useState<AutonomyPolicy[]>([]);
  const [resolved, setResolved] = useState<AutonomyPolicy | null>(null);
  const [resolvedKind, setResolvedKind] = useState<string>('');
  const [busy, setBusy] = useState(false);
  const [reloadTick, setReloadTick] = useState(0);

  const triggerReload = useCallback(() => setReloadTick((n) => n + 1), []);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const list = await getJson<ListResponse | AutonomyPolicy[]>(
          `/autonomy_policies?project_id=${encodeURIComponent(projectId)}`,
        );
        if (cancelled) return;
        const rows = Array.isArray(list) ? list : list.policies;
        setPolicies(rows);
        const qs = new URLSearchParams({ project_id: projectId });
        if (planId) qs.set('plan_id', planId);
        if (resolvedKind) qs.set('decision_kind', resolvedKind);
        const r = await getJson<ResolveResponse>(`/autonomy_policies/resolve?${qs}`);
        if (cancelled) return;
        setResolved(r.policy);
      } catch (err) {
        if (err instanceof DaemonError) toastError(`${err.code}: ${err.message}`);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [projectId, planId, resolvedKind, refreshKey, reloadTick]);

  async function setMode(scope: AutonomyScopeKind, mode: AutonomyMode, decisionKind?: string) {
    setBusy(true);
    try {
      await postJson('/autonomy_policies', {
        project_id: projectId,
        scope_kind: scope,
        plan_id: scope === 'plan' ? planId : null,
        decision_kind: scope === 'decision_kind' ? decisionKind : null,
        mode,
        set_by: 'web',
      });
      toastSuccess(`Autonomy → ${mode}`);
      triggerReload();
    } catch (err) {
      if (err instanceof DaemonError) toastError(`${err.code}: ${err.message}`);
      else toastError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function circuitBreaker() {
    const reason = window.prompt(
      'Demote every policy in this project to L3. Reason (required):',
    );
    if (!reason || !reason.trim()) return;
    setBusy(true);
    try {
      const r = await postJson<{ demoted: number }>(
        '/autonomy_policies/circuit_breaker',
        { project_id: projectId, actor: 'web', reason: reason.trim() },
      );
      toastSuccess(`Circuit breaker fired — ${r.demoted} policies demoted to L3`);
      triggerReload();
    } catch (err) {
      if (err instanceof DaemonError) toastError(`${err.code}: ${err.message}`);
      else toastError(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="space-y-3">
      <div className="flex items-center justify-between">
        <h3 className="text-headline-md text-foreground flex items-baseline gap-2">
          Autonomy
          <span className="text-xs text-muted font-normal">{policies.length}</span>
        </h3>
        <Button
          size="sm"
          variant="outline"
          onClick={circuitBreaker}
          disabled={busy}
          title="Demote all policies to L3 (D18)"
          className="border-danger/40 text-danger hover:bg-danger/10"
        >
          ⚠ Circuit breaker
        </Button>
      </div>

      <div className="rounded-md border border-border bg-background p-3 space-y-2">
        <div className="text-[10px] uppercase tracking-wide text-muted">
          Resolved policy {planId ? `for plan ${planId}` : '(global)'}
        </div>
        <div className="flex items-center gap-2 flex-wrap">
          {resolved ? (
            <>
              <Badge size="md" variant={MODE_VARIANT[resolved.mode]}>
                {MODE_LABEL[resolved.mode]}
              </Badge>
              <span className="text-xs text-muted">
                scope=<span className="font-mono">{resolved.scope_kind}</span>
                {resolved.decision_kind ? (
                  <> · kind=<span className="font-mono">{resolved.decision_kind}</span></>
                ) : null}
                {resolved.reason ? <> · {resolved.reason}</> : null}
              </span>
            </>
          ) : (
            <span className="text-xs text-muted italic">no policy — daemon default applies</span>
          )}
        </div>
        <div className="flex items-center gap-2">
          <span className="text-[11px] text-muted">Resolve for decision-kind:</span>
          <Input
            size="sm"
            value={resolvedKind}
            onChange={(e) => setResolvedKind(e.target.value)}
            placeholder="(empty = generic)"
            className="max-w-[200px] font-mono text-xs"
          />
        </div>
      </div>

      {planId && (
        <ModeSlider
          label={`Plan ${planId}`}
          mode={planLevelMode(policies, planId)}
          onChange={(m) => setMode('plan', m)}
          busy={busy}
        />
      )}
      <ModeSlider
        label="Global default"
        mode={globalMode(policies)}
        onChange={(m) => setMode('global', m)}
        busy={busy}
      />

      <DecisionKindMatrix policies={policies} onChange={(kind, m) => setMode('decision_kind', m, kind)} busy={busy} />

      {policies.length > 0 && (
        <details className="text-xs">
          <summary className="cursor-pointer text-muted hover:text-foreground">
            All policies ({policies.length})
          </summary>
          <ul className="mt-2 divide-y divide-border rounded-md border border-border bg-background">
            {policies.map((p) => (
              <li key={p.id} className="px-3 py-2 flex items-center gap-2">
                <Badge size="sm" variant={MODE_VARIANT[p.mode]}>{p.mode}</Badge>
                <span className="font-mono text-[10px] text-muted">
                  {p.scope_kind}
                  {p.plan_id ? ` · ${p.plan_id}` : ''}
                  {p.decision_kind ? ` · ${p.decision_kind}` : ''}
                </span>
                <span className="ml-auto text-[10px] text-muted">
                  by {p.set_by} · {new Date(p.set_at).toLocaleString()}
                </span>
              </li>
            ))}
          </ul>
        </details>
      )}
    </section>
  );
}

function planLevelMode(policies: AutonomyPolicy[], planId: string | null | undefined): AutonomyMode | null {
  if (!planId) return null;
  return policies.find((p) => p.scope_kind === 'plan' && p.plan_id === planId)?.mode ?? null;
}

function globalMode(policies: AutonomyPolicy[]): AutonomyMode | null {
  return policies.find((p) => p.scope_kind === 'global')?.mode ?? null;
}

function ModeSlider({
  label,
  mode,
  onChange,
  busy,
}: {
  label: string;
  mode: AutonomyMode | null;
  onChange: (mode: AutonomyMode) => void;
  busy: boolean;
}) {
  const modes: AutonomyMode[] = ['L3', 'L4', 'L5'];
  return (
    <div className="rounded-md border border-border bg-background p-3 space-y-2">
      <div className="flex items-center justify-between">
        <span className="text-xs text-foreground font-medium truncate">{label}</span>
        {mode ? (
          <Badge size="sm" variant={MODE_VARIANT[mode]}>{mode}</Badge>
        ) : (
          <span className="text-[10px] text-muted italic">unset</span>
        )}
      </div>
      <div className="flex gap-1">
        {modes.map((m) => (
          <button
            key={m}
            type="button"
            onClick={() => onChange(m)}
            disabled={busy}
            aria-pressed={mode === m}
            className={cn(
              'flex-1 rounded px-2 py-1 text-xs font-medium border transition-colors cursor-pointer',
              mode === m
                ? 'bg-primary text-primary-foreground border-primary'
                : 'border-border text-muted hover:text-foreground hover:bg-surface-high/60',
              busy && 'opacity-50 cursor-not-allowed',
            )}
          >
            {MODE_LABEL[m]}
          </button>
        ))}
      </div>
    </div>
  );
}

function DecisionKindMatrix({
  policies,
  onChange,
  busy,
}: {
  policies: AutonomyPolicy[];
  onChange: (kind: string, mode: AutonomyMode) => void;
  busy: boolean;
}) {
  const [newKind, setNewKind] = useState('');
  const byKind = new Map<string, AutonomyMode>();
  for (const p of policies) {
    if (p.scope_kind === 'decision_kind' && p.decision_kind) {
      byKind.set(p.decision_kind, p.mode);
    }
  }
  const kinds = Array.from(byKind.keys()).sort();
  const known = new Set(kinds);
  const hint = ['architecture', 'schema', 'naming-canonical'].filter((k) => !known.has(k));

  return (
    <div className="rounded-md border border-border bg-background p-3 space-y-2">
      <div className="text-xs text-foreground font-medium">By decision-kind (D17)</div>
      {kinds.length === 0 && (
        <p className="text-[11px] text-muted italic">No per-kind overrides set.</p>
      )}
      {kinds.map((k) => {
        const mode = byKind.get(k) ?? 'L4';
        const forced = FORCED_L4_KINDS.has(k);
        return (
          <div key={k} className="flex items-center gap-2">
            <span className="font-mono text-[11px] text-foreground w-32 truncate">{k}</span>
            {forced && <Badge size="sm" variant="warning">forced ≥ L4</Badge>}
            <Select
              size="sm"
              value={mode}
              onChange={(e) => onChange(k, e.target.value as AutonomyMode)}
              disabled={busy}
              className="max-w-[140px]"
            >
              {(['L3', 'L4', 'L5'] as AutonomyMode[]).map((m) => (
                <option key={m} value={m} disabled={forced && m === 'L5'}>
                  {MODE_LABEL[m]}
                </option>
              ))}
            </Select>
          </div>
        );
      })}
      <div className="flex items-center gap-2 pt-1">
        <Input
          size="sm"
          value={newKind}
          onChange={(e) => setNewKind(e.target.value)}
          placeholder="e.g. architecture"
          list="autonomy-known-kinds"
          className="max-w-[200px] font-mono text-xs"
        />
        <datalist id="autonomy-known-kinds">
          {hint.map((k) => (
            <option key={k} value={k} />
          ))}
        </datalist>
        <Select
          size="sm"
          defaultValue="L4"
          id="autonomy-new-kind-mode"
          className="max-w-[120px]"
        >
          {(['L3', 'L4', 'L5'] as AutonomyMode[]).map((m) => (
            <option key={m} value={m}>{m}</option>
          ))}
        </Select>
        <Button
          size="sm"
          variant="outline"
          disabled={busy || !newKind.trim()}
          onClick={() => {
            const sel = document.getElementById('autonomy-new-kind-mode') as HTMLSelectElement | null;
            const m = (sel?.value as AutonomyMode) ?? 'L4';
            onChange(newKind.trim(), m);
            setNewKind('');
          }}
        >
          add
        </Button>
      </div>
    </div>
  );
}
