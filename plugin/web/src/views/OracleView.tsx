import { useEffect, useState } from 'react';
import type { OracleVerify, SsotNode, UserFlow } from '../types/entities';
import {
  getOracleVerify,
  listSsotNodes,
  listUserFlows,
} from '../lib/api';
import { Badge } from '../components/ui';
import { cn } from '../lib/cn';

interface OracleViewProps {
  projectId: string;
  refreshKey: number;
}

/** D34/D35 — oracle completeness dashboard. Visualizes the daemon's
 *  deterministic `verify` verdict: L0 (facet-incomplete nodes + dangling
 *  edges), L1 (uncovered persona × capability pairs), the open-question count,
 *  the L2 enforcement flag, and the aggregate `oracle_complete` gate. The
 *  numbers are authoritative from the daemon; this view never re-derives them. */
export function OracleView({ projectId, refreshKey }: OracleViewProps) {
  const [verify, setVerify] = useState<OracleVerify | null>(null);
  const [nodes, setNodes] = useState<SsotNode[]>([]);
  const [flows, setFlows] = useState<UserFlow[]>([]);
  const [error, setError] = useState<string | null>(null);
  const loading = verify === null && error === null;

  useEffect(() => {
    let cancelled = false;
    Promise.all([
      getOracleVerify(projectId),
      listSsotNodes(projectId).catch(() => [] as SsotNode[]),
      listUserFlows(projectId).catch(() => [] as UserFlow[]),
    ])
      .then(([v, n, f]) => {
        if (cancelled) return;
        setVerify(v);
        setNodes(n);
        setFlows(f);
        setError(null);
      })
      .catch((err) => {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [projectId, refreshKey]);

  const personaCount = nodes.filter((n) => n.kind === 'Persona').length;
  const capabilityCount = nodes.filter((n) => n.kind === 'Capability').length;
  const confirmedFlows = flows.filter((f) => f.status === 'confirmed').length;

  // Gate tiers that make up the verdict — each contributes a step to the
  // progress meter. L2 only counts when the daemon reports it enforced.
  const tiers = verify
    ? [
        { key: 'L0', complete: verify.l0.complete },
        { key: 'L1', complete: verify.l1.complete },
        { key: 'Q', complete: verify.questions.clear },
        ...(verify.l2.enforced ? [{ key: 'L2', complete: false }] : []),
      ]
    : [];
  const passed = tiers.filter((t) => t.complete).length;
  const pct = tiers.length > 0 ? Math.round((passed / tiers.length) * 100) : 0;

  return (
    <div className="h-full flex flex-col">
      <header className="px-6 pt-5 pb-3 border-b border-border">
        <div className="flex items-center gap-3 flex-wrap">
          <h2 className="text-headline-lg text-foreground">Oracle</h2>
          {verify && (
            <Badge
              size="md"
              variant={verify.oracle_complete ? 'success' : 'warning'}
            >
              {verify.oracle_complete ? '✓ oracle complete' : 'incomplete'}
            </Badge>
          )}
        </div>
        <p className="text-sm text-muted mt-1">
          제품 정의 그래프의 결정적 완전성 — L0 측면/연결, L1 커버리지, 미답 질문.
        </p>
      </header>

      <div className="flex-1 overflow-auto p-6 space-y-6">
        {error && <p className="text-sm text-danger">{error}</p>}
        {!error && loading && <p className="text-sm text-muted">로딩…</p>}

        {verify && (
          <>
            {/* Aggregate progress meter. */}
            <section className="rounded-lg border border-border bg-surface p-4 space-y-3">
              <div className="flex items-center justify-between">
                <span className="text-[10px] uppercase tracking-wide text-muted">
                  완전성 진행률
                </span>
                <span className="text-sm font-mono text-foreground">
                  {passed} / {tiers.length} ({pct}%)
                </span>
              </div>
              <div className="h-2.5 w-full overflow-hidden rounded-full bg-background">
                <div
                  className={cn(
                    'h-full rounded-full transition-all',
                    verify.oracle_complete ? 'bg-success' : 'bg-warning',
                  )}
                  style={{ width: `${pct}%` }}
                />
              </div>
              <div className="flex flex-wrap gap-2">
                {tiers.map((t) => (
                  <Badge
                    key={t.key}
                    size="sm"
                    variant={t.complete ? 'success' : 'warning'}
                  >
                    {t.complete ? '✓' : '○'} {t.key}
                  </Badge>
                ))}
              </div>
            </section>

            <div className="grid gap-4 md:grid-cols-3">
              <StatCard
                label="L0 · 측면 미완 노드"
                value={verify.l0.facet_incomplete_nodes}
                ok={verify.l0.facet_incomplete_nodes === 0}
              />
              <StatCard
                label="L0 · dangling 엣지"
                value={verify.l0.dangling_edges}
                ok={verify.l0.dangling_edges === 0}
              />
              <StatCard
                label="미답 질문"
                value={verify.questions.open}
                ok={verify.questions.clear}
              />
            </div>

            {/* L1 — uncovered persona × capability pairs. */}
            <section className="space-y-2">
              <div className="flex items-center gap-2">
                <span className="text-[10px] uppercase tracking-wide text-muted">
                  L1 · 미커버 Persona × Capability
                </span>
                <Badge
                  size="sm"
                  variant={verify.l1.complete ? 'success' : 'warning'}
                >
                  {verify.l1.complete
                    ? '커버리지 100%'
                    : `${verify.l1.uncovered_persona_capability_pairs.length} 미커버`}
                </Badge>
                <span className="text-[11px] text-muted">
                  Persona {personaCount} · Capability {capabilityCount} · 확정 flow{' '}
                  {confirmedFlows}
                </span>
              </div>
              {verify.l1.complete ? (
                <p className="text-sm text-success">
                  모든 (Persona × Capability) 가 ≥1 확정 flow 로 커버됨.
                </p>
              ) : (
                <ul className="space-y-1.5">
                  {verify.l1.uncovered_persona_capability_pairs.map((p) => (
                    <li
                      key={`${p.persona}:${p.capability}`}
                      className="rounded-md border border-warning/40 bg-warning/5 px-3 py-2 text-sm"
                    >
                      <span className="text-foreground">{p.persona_title}</span>
                      <span className="mx-2 text-muted">×</span>
                      <span className="text-foreground">{p.capability_title}</span>
                      <span className="ml-2 text-[10px] font-mono text-muted">
                        {p.persona} · {p.capability}
                      </span>
                    </li>
                  ))}
                </ul>
              )}
            </section>

            {/* L2 enforcement note (computed in Phase 4b on the daemon). */}
            <section className="rounded-md border border-border bg-surface px-3 py-2">
              <span className="text-[11px] text-muted">
                L2 (flow-step → DetailScenario) 커버리지:{' '}
                {verify.l2.enforced ? (
                  <span className="text-foreground">enforced</span>
                ) : (
                  <span>아직 미강제 — plan approve 게이트 재작성과 함께 4b 에서 활성</span>
                )}
              </span>
            </section>
          </>
        )}
      </div>
    </div>
  );
}

function StatCard({
  label,
  value,
  ok,
}: {
  label: string;
  value: number;
  ok: boolean;
}) {
  return (
    <div
      className={cn(
        'rounded-lg border p-4',
        ok ? 'border-success/40 bg-success/5' : 'border-warning/40 bg-warning/5',
      )}
    >
      <div className="text-[10px] uppercase tracking-wide text-muted">{label}</div>
      <div
        className={cn(
          'mt-1 text-2xl font-mono',
          ok ? 'text-success' : 'text-warning',
        )}
      >
        {value}
      </div>
    </div>
  );
}
