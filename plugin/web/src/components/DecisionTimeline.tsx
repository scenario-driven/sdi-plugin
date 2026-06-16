import { useMemo } from 'react';
import type { Decision, DecisionKind } from '../types/entities';
import { Badge } from './ui';
import { cn } from '../lib/cn';

interface DecisionTimelineProps {
  decisions: Decision[];
  onSelect: (id: string) => void;
}

interface ProposalRow {
  proposal: Decision | null;
  proposalId: string;
  critiques: Decision[];
  consensus: Decision | null;
  dissensus: Decision | null;
  stage: DecisionKind;
}

const KIND_VARIANT: Record<DecisionKind, 'default' | 'primary' | 'success' | 'warning' | 'danger'> = {
  proposal: 'primary',
  critique: 'warning',
  consensus: 'success',
  dissensus: 'danger',
};

const STAGE_ORDER: DecisionKind[] = ['proposal', 'critique', 'consensus', 'dissensus'];

const STAGE_REACHED_CLASS: Record<DecisionKind, string> = {
  proposal: 'bg-primary/10 text-primary',
  critique: 'bg-warning/10 text-warning',
  consensus: 'bg-success/10 text-success',
  dissensus: 'bg-danger/10 text-danger',
};

export function DecisionTimeline({ decisions, onSelect }: DecisionTimelineProps) {
  const rows = useMemo(() => groupByProposal(decisions), [decisions]);

  if (rows.length === 0) {
    return <p className="text-xs italic text-muted px-2 py-2">No decisions.</p>;
  }

  return (
    <ul className="space-y-3">
      {rows.map((row) => (
        <li
          key={row.proposalId}
          className="rounded-md border border-border bg-background overflow-hidden"
        >
          <StageStrip row={row} />
          <div className="px-3 py-2 space-y-2">
            {row.proposal ? (
              <DecisionLine d={row.proposal} onSelect={onSelect} />
            ) : (
              <div className="text-[11px] text-muted italic">
                proposal {row.proposalId} not loaded
              </div>
            )}
            {row.critiques.length > 0 && (
              <div className="border-l-2 border-warning/40 pl-2 space-y-1">
                {row.critiques.map((c) => (
                  <DecisionLine key={c.id} d={c} onSelect={onSelect} />
                ))}
              </div>
            )}
            {row.consensus && (
              <div className="border-l-2 border-success/40 pl-2">
                <DecisionLine d={row.consensus} onSelect={onSelect} />
              </div>
            )}
            {row.dissensus && (
              <div className="border-l-2 border-danger/60 pl-2">
                <DecisionLine d={row.dissensus} onSelect={onSelect} />
                <div className="text-[10px] text-danger">
                  escalated{row.dissensus.escalated_at
                    ? ` ${new Date(row.dissensus.escalated_at).toLocaleString()}`
                    : ''}
                </div>
              </div>
            )}
          </div>
        </li>
      ))}
    </ul>
  );
}

function StageStrip({ row }: { row: ProposalRow }) {
  return (
    <div className="flex items-stretch text-[10px] uppercase tracking-wide">
      {STAGE_ORDER.map((stage) => {
        const reached = stageReached(row, stage);
        const current = row.stage === stage;
        return (
          <div
            key={stage}
            className={cn(
              'flex-1 px-2 py-1 text-center transition-colors',
              reached ? STAGE_REACHED_CLASS[stage] : 'bg-muted/5 text-muted',
              current && 'font-bold ring-1 ring-inset ring-current',
            )}
          >
            {stage}
            {stage === 'critique' && row.critiques.length > 1 ? ` ×${row.critiques.length}` : ''}
          </div>
        );
      })}
    </div>
  );
}

function stageReached(row: ProposalRow, stage: DecisionKind): boolean {
  switch (stage) {
    case 'proposal':
      return true;
    case 'critique':
      return row.critiques.length > 0;
    case 'consensus':
      return !!row.consensus;
    case 'dissensus':
      return !!row.dissensus;
  }
}

function DecisionLine({ d, onSelect }: { d: Decision; onSelect: (id: string) => void }) {
  return (
    <button
      type="button"
      onClick={() => onSelect(d.id)}
      className="w-full text-left rounded px-2 py-1 hover:bg-surface-high/60 transition-colors cursor-pointer"
    >
      <div className="flex items-center gap-2 flex-wrap">
        <Badge size="sm" variant={KIND_VARIANT[d.kind]}>{d.kind}</Badge>
        {d.supersede_when && (
          <Badge size="sm" variant="warning">provisional</Badge>
        )}
        <span className="text-[10px] font-mono text-muted">{d.short_code}</span>
        {d.agent_name && (
          <span className="text-[10px] text-muted">
            by <span className="font-mono">{d.agent_name}</span>
          </span>
        )}
        <span className="text-[10px] text-muted ml-auto">
          {new Date(d.created_at).toLocaleString()}
        </span>
      </div>
      <div className="text-xs font-medium text-foreground">{d.title}</div>
      {d.supersede_when && (
        <div className="text-[10px] text-warning mt-0.5">
          revisit when: {d.supersede_when}
        </div>
      )}
    </button>
  );
}

function groupByProposal(decisions: Decision[]): ProposalRow[] {
  const rows = new Map<string, ProposalRow>();
  const ensure = (proposalId: string): ProposalRow => {
    let row = rows.get(proposalId);
    if (!row) {
      row = {
        proposal: null,
        proposalId,
        critiques: [],
        consensus: null,
        dissensus: null,
        stage: 'proposal',
      };
      rows.set(proposalId, row);
    }
    return row;
  };

  for (const d of decisions) {
    if (d.kind === 'proposal') {
      const row = ensure(d.id);
      row.proposal = d;
    } else {
      const pid = d.proposal_id ?? '';
      if (!pid) continue;
      const row = ensure(pid);
      switch (d.kind) {
        case 'critique':
          row.critiques.push(d);
          break;
        case 'consensus':
          row.consensus = d;
          break;
        case 'dissensus':
          row.dissensus = d;
          break;
      }
    }
  }

  for (const row of rows.values()) {
    if (row.dissensus) row.stage = 'dissensus';
    else if (row.consensus) row.stage = 'consensus';
    else if (row.critiques.length > 0) row.stage = 'critique';
    else row.stage = 'proposal';
    row.critiques.sort(byCreatedAt);
  }

  return Array.from(rows.values()).sort((a, b) => {
    const aTime = a.proposal?.created_at ?? a.critiques[0]?.created_at ?? '';
    const bTime = b.proposal?.created_at ?? b.critiques[0]?.created_at ?? '';
    return aTime < bTime ? 1 : aTime > bTime ? -1 : 0;
  });
}

function byCreatedAt(a: Decision, b: Decision): number {
  return a.created_at < b.created_at ? -1 : a.created_at > b.created_at ? 1 : 0;
}
