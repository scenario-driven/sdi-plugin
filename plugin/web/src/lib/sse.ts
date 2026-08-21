import { useEffect, useRef } from 'react';
import type { SdiEvent } from '../types/entities';
import { daemonUrl } from './daemonUrl';

/** v0.5 SSE event kinds emitted by `sdid` for Layer 2.6/2.7/2.8 surfaces.
 *  Underscore-separated to match daemon serialization (see
 *  ARCHITECTURE.md §"How the surfaces render this"). */
export const PATTERN_EVENT_KINDS = [
  'pattern_created',
  'pattern_lifecycle',
  'pattern_aborted',
] as const;

export const REVERSIBILITY_EVENT_KINDS = [
  'rollback_initiated',
  'rollback_completed',
] as const;

export const CLAIM_EVENT_KINDS = ['claim_status_changed'] as const;

export type PatternEventKind = (typeof PATTERN_EVENT_KINDS)[number];
export type ReversibilityEventKind = (typeof REVERSIBILITY_EVENT_KINDS)[number];
export type ClaimEventKind = (typeof CLAIM_EVENT_KINDS)[number];

const PATTERN_EVENT_SET = new Set<string>(PATTERN_EVENT_KINDS);
const REVERSIBILITY_EVENT_SET = new Set<string>(REVERSIBILITY_EVENT_KINDS);
const CLAIM_EVENT_SET = new Set<string>(CLAIM_EVENT_KINDS);

export function isPatternEvent(ev: SdiEvent): boolean {
  return PATTERN_EVENT_SET.has(ev.kind);
}

export function isReversibilityEvent(ev: SdiEvent): boolean {
  return REVERSIBILITY_EVENT_SET.has(ev.kind);
}

export function isClaimEvent(ev: SdiEvent): boolean {
  return CLAIM_EVENT_SET.has(ev.kind);
}

// Subscribe to `/events`. The daemon emits a single stream of typed envelopes;
// callers usually filter by `kind`. Reconnects on disconnect with a small
// backoff because EventSource closes the stream on idle Wi-Fi resume.
//
// SSE never flows through the Vite proxy in dev (the proxy buffers chunks).
// We call `daemonUrl('/events')` so the request lands directly on the daemon
// origin during `vite dev` and collapses to same-origin in production.

const LAST_EVENT_ID_KEY = 'sdi.sse.lastEventId';

function readLastEventId(): string | null {
  try {
    return localStorage.getItem(LAST_EVENT_ID_KEY);
  } catch {
    return null;
  }
}

function writeLastEventId(id: string): void {
  try {
    localStorage.setItem(LAST_EVENT_ID_KEY, id);
  } catch {
    /* private mode / quota — ignore */
  }
}

export function useSdiEvents(
  onEvent: (event: SdiEvent) => void,
  filter?: (event: SdiEvent) => boolean,
): void {
  const handlerRef = useRef(onEvent);
  const filterRef = useRef(filter);

  useEffect(() => {
    handlerRef.current = onEvent;
    filterRef.current = filter;
  }, [onEvent, filter]);

  useEffect(() => {
    let source: EventSource | null = null;
    let reconnect: ReturnType<typeof setTimeout> | null = null;
    let alive = true;

    const open = () => {
      if (!alive) return;
      const lastId = readLastEventId();
      const suffix = lastId ? `?last_event_id=${encodeURIComponent(lastId)}` : '';
      source = new EventSource(daemonUrl(`/events${suffix}`), {
        withCredentials: true,
      });
      source.onmessage = (msg) => {
        if (msg.lastEventId) writeLastEventId(msg.lastEventId);
        if (!msg.data) return;
        try {
          const parsed = JSON.parse(msg.data) as SdiEvent;
          if (filterRef.current && !filterRef.current(parsed)) return;
          handlerRef.current(parsed);
        } catch {
          // Malformed envelope — daemon contract violation; ignore at the UI
          // level rather than crashing the dashboard.
        }
      };
      source.onerror = () => {
        source?.close();
        source = null;
        if (alive) reconnect = setTimeout(open, 1500);
      };
    };

    open();
    return () => {
      alive = false;
      if (reconnect) clearTimeout(reconnect);
      source?.close();
    };
  }, []);
}
