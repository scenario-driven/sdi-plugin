import { useEffect, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import type { Decision, DecisionKind, DecisionStatus } from '../types/entities';
import { getJson, postJson, DaemonError } from '../lib/api';
import { toastError, toastSuccess } from '../lib/toast';
import { Badge, Button } from './ui';

const STATUS_VARIANT: Record<DecisionStatus, 'default' | 'primary' | 'success'> = {
  proposed: 'default',
  accepted: 'primary',
  superseded: 'default',
};

const KIND_VARIANT: Record<DecisionKind, 'default' | 'primary' | 'success' | 'warning' | 'danger'> = {
  proposal: 'primary',
  critique: 'warning',
  consensus: 'success',
  dissensus: 'danger',
};

interface DecisionDetailProps {
  decisionId: string;
  onClose: () => void;
  refreshKey: number;
}

export function DecisionDetail({ decisionId, onClose, refreshKey }: DecisionDetailProps) {
  const [decision, setDecision] = useState<Decision | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const d = await getJson<Decision>(`/decisions/${decisionId}`).catch(async () => {
          const list = await getJson<Decision[]>(`/decisions?id=${decisionId}`).catch(() => [] as Decision[]);
          return list[0] ?? null;
        });
        if (!d) throw new Error('Decision not found');
        if (!cancelled) setDecision(d);
      } catch (err) {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [decisionId, refreshKey]);

  async function supersede() {
    if (!decision) return;
    setBusy(true);
    try {
      await postJson(`/decisions/${decision.id}/status`, { status: 'superseded' });
      toastSuccess('Decision superseded');
    } catch (err) {
      if (err instanceof DaemonError) toastError(`${err.code}: ${err.message}`);
      else toastError(String(err));
    } finally {
      setBusy(false);
    }
  }

  if (error) return <Wrapper onClose={onClose}><p className="text-danger text-sm">{error}</p></Wrapper>;
  if (!decision) return <Wrapper onClose={onClose}><p className="text-sm text-muted">Loading…</p></Wrapper>;

  return (
    <Wrapper onClose={onClose}>
      <header className="space-y-2">
        <div className="flex items-center gap-2 flex-wrap">
          <span className="text-xs font-mono text-muted">{decision.short_code}</span>
          <Badge size="sm" variant={KIND_VARIANT[decision.kind]}>{decision.kind}</Badge>
          <Badge size="sm" variant={STATUS_VARIANT[decision.status]}>{decision.status}</Badge>
          {decision.escalated_at && (
            <Badge size="sm" variant="danger">escalated</Badge>
          )}
        </div>
        <h2 className="text-headline-md text-foreground">{decision.title}</h2>
        {decision.agent_name && (
          <p className="text-[11px] text-muted">
            by <span className="font-mono">{decision.agent_name}</span>
          </p>
        )}
      </header>

      {decision.status === 'accepted' && (
        <Button size="sm" variant="outline" onClick={supersede} disabled={busy}>
          {busy ? '…' : 'Mark superseded'}
        </Button>
      )}

      <div className="rounded-md border border-border bg-background p-4">
        <div className="prose prose-sm max-w-none text-foreground prose-headings:text-foreground prose-strong:text-foreground prose-code:text-foreground prose-a:text-primary">
          <ReactMarkdown remarkPlugins={[remarkGfm]}>{decision.body}</ReactMarkdown>
        </div>
      </div>

      <section className="text-[11px] text-muted space-y-0.5">
        <div>Created {new Date(decision.created_at).toLocaleString()}</div>
        {decision.proposal_id && (
          <div>
            Proposal <span className="font-mono">{decision.proposal_id}</span>
          </div>
        )}
        {decision.supersedes_id && (
          <div>Supersedes <span className="font-mono">{decision.supersedes_id}</span></div>
        )}
        {decision.escalated_at && (
          <div className="text-danger">
            Escalated {new Date(decision.escalated_at).toLocaleString()}
          </div>
        )}
      </section>
    </Wrapper>
  );
}

function Wrapper({ onClose, children }: { onClose: () => void; children: React.ReactNode }) {
  return (
    <div className="h-full flex flex-col">
      <div className="px-5 py-3 border-b border-border flex items-center justify-between sticky top-0 bg-surface z-10">
        <span className="text-[10px] uppercase tracking-wide text-muted">Decision</span>
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
