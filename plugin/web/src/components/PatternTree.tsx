import { useEffect, useMemo, useState } from 'react';
import type {
  CollaborationPattern,
  PatternKind,
  PatternLifecycle,
} from '../types/entities';
import { DaemonError, listPatterns } from '../lib/api';
import { toastError } from '../lib/toast';
import { Badge } from './ui';
import { cn } from '../lib/cn';

interface PatternTreeProps {
  planId: string;
  refreshKey: number;
  /** D24 — AutonomyPolicy.pattern_depth_cap (default 3). Renders a warning
   *  badge on any node whose `depth` already meets or exceeds the cap so
   *  the user sees what the daemon will reject on the next spawn. */
  depthCap?: number;
  onSelect?: (pattern: CollaborationPattern) => void;
}

const KIND_VARIANT: Record<PatternKind, 'default' | 'primary' | 'success' | 'warning' | 'danger' | 'secondary' | 'info'> = {
  workflow: 'primary',
  graph: 'secondary',
  swarm: 'info',
  'agents-as-tools': 'success',
  direct: 'danger',
};

const LIFECYCLE_VARIANT: Record<PatternLifecycle, 'default' | 'primary' | 'success' | 'warning' | 'danger'> = {
  pending: 'default',
  active: 'primary',
  converged: 'success',
  dissensus: 'warning',
  aborted: 'danger',
};

/** Defensive client-side render cap. The daemon's topological sort already
 *  prevents cycles (D24) but if a malformed payload ever sneaks through we
 *  stop the recursion at this absolute ceiling instead of locking the tab. */
const ABSOLUTE_RENDER_CEILING = 10;

export function PatternTree({
  planId,
  refreshKey,
  depthCap = 3,
  onSelect,
}: PatternTreeProps) {
  const [patterns, setPatterns] = useState<CollaborationPattern[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const rows = await listPatterns(planId);
        if (cancelled) return;
        setPatterns(rows);
      } catch (err) {
        if (cancelled) return;
        if (err instanceof DaemonError) {
          toastError(`${err.code}: ${err.message}`);
          setError(err.message);
        } else {
          setError(err instanceof Error ? err.message : String(err));
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [planId, refreshKey]);

  const roots = useMemo(() => buildRoots(patterns), [patterns]);

  if (error) return <p className="text-xs text-danger px-2 py-2">{error}</p>;
  if (patterns.length === 0) {
    return (
      <p className="text-xs italic text-muted px-2 py-6 text-center">
        No CollaborationPattern rows on this plan yet.
      </p>
    );
  }

  return (
    <section className="space-y-3">
      <header className="flex items-center gap-2">
        <h3 className="text-headline-md text-foreground flex items-baseline gap-2">
          Pattern tree
          <span className="text-xs text-muted font-normal">{patterns.length}</span>
        </h3>
        <span className="text-[10px] text-muted ml-auto">
          depth cap = {depthCap}
        </span>
      </header>
      <ul className="space-y-1">
        {roots.map((node) => (
          <TreeNode
            key={node.pattern.id}
            node={node}
            depthCap={depthCap}
            renderDepth={0}
            onSelect={onSelect}
          />
        ))}
      </ul>
    </section>
  );
}

interface TreeRow {
  pattern: CollaborationPattern;
  children: TreeRow[];
}

function buildRoots(patterns: CollaborationPattern[]): TreeRow[] {
  const byId = new Map<string, TreeRow>();
  for (const p of patterns) byId.set(p.id, { pattern: p, children: [] });
  const roots: TreeRow[] = [];
  for (const node of byId.values()) {
    const parentId = node.pattern.parent_pattern_id;
    if (parentId && byId.has(parentId)) {
      byId.get(parentId)!.children.push(node);
    } else {
      roots.push(node);
    }
  }
  // Deterministic order: by depth then created_at.
  const sortRec = (rows: TreeRow[]) => {
    rows.sort((a, b) => {
      if (a.pattern.depth !== b.pattern.depth) {
        return a.pattern.depth - b.pattern.depth;
      }
      return a.pattern.created_at < b.pattern.created_at ? -1 : 1;
    });
    for (const r of rows) sortRec(r.children);
  };
  sortRec(roots);
  return roots;
}

interface TreeNodeProps {
  node: TreeRow;
  depthCap: number;
  renderDepth: number;
  onSelect?: (pattern: CollaborationPattern) => void;
}

function TreeNode({ node, depthCap, renderDepth, onSelect }: TreeNodeProps) {
  const { pattern, children } = node;
  const overCap = pattern.depth >= depthCap;
  const renderHalted = renderDepth >= ABSOLUTE_RENDER_CEILING;

  return (
    <li>
      <div
        className={cn(
          'rounded-md border border-border bg-background px-3 py-2',
          'transition-colors',
          onSelect && 'cursor-pointer hover:bg-surface-high',
          pattern.kind === 'direct' && 'border-l-4 border-l-danger',
        )}
        style={{ marginLeft: `${renderDepth * 16}px` }}
        onClick={onSelect ? () => onSelect(pattern) : undefined}
      >
        <div className="flex items-center gap-2 flex-wrap">
          <span className="text-[10px] font-mono text-muted">{pattern.short_code}</span>
          <Badge size="sm" variant={KIND_VARIANT[pattern.kind]}>{pattern.kind}</Badge>
          <span className="text-[10px] text-muted">
            <span className="font-mono">{pattern.applies_to}</span>
            <span> · </span>
            <span className="font-mono">{pattern.scope_id.slice(-8)}</span>
          </span>
          <Badge size="sm" variant={LIFECYCLE_VARIANT[pattern.lifecycle]}>
            {pattern.lifecycle}
          </Badge>
          <span
            className={cn(
              'text-[10px] font-mono',
              overCap ? 'text-danger font-semibold' : 'text-muted',
            )}
            title={overCap ? `Depth ≥ cap (${depthCap}) — sub-pattern spawn blocked` : undefined}
          >
            depth={pattern.depth}
          </span>
          {overCap && (
            <Badge size="sm" variant="danger">depth cap reached</Badge>
          )}
        </div>
      </div>
      {renderHalted ? (
        <p
          className="ml-4 mt-1 text-[10px] italic text-danger"
          style={{ marginLeft: `${(renderDepth + 1) * 16}px` }}
        >
          render halted — exceeded {ABSOLUTE_RENDER_CEILING} levels (daemon cycle guard tripped?)
        </p>
      ) : (
        children.length > 0 && (
          <ul className="mt-1 space-y-1">
            {children.map((child) => (
              <TreeNode
                key={child.pattern.id}
                node={child}
                depthCap={depthCap}
                renderDepth={renderDepth + 1}
                onSelect={onSelect}
              />
            ))}
          </ul>
        )
      )}
    </li>
  );
}
