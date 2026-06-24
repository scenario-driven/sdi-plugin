// Conversational-mode LLM client (PRD-v2 D36 "대화 모드" + D37 llm-bridge).
//
// The daemon exposes `POST /v1/agent/stream` (SSE response) as a thin proxy to
// the Node `llm-bridge` sidecar, which reuses the local Claude subscription via
// ~/.claude OAuth (API cost 0). Because the endpoint is POST-with-SSE-response,
// the browser's `EventSource` (GET-only) cannot drive it; we read the response
// body as a stream and parse the `data:` frames ourselves.
//
// Provider selection is per-request (D37): "sdk" is stateless (natural fit for
// one-shot batch), "acp" keeps a stateful session keyed by `clientSessionId`
// (natural fit for a back-and-forth discussion). The dashboard's chat mode uses
// "acp" so successive turns about the same question share context.

import { authHeaders } from './auth';
import { daemonUrl } from './daemonUrl';

export type AgentProvider = 'acp' | 'sdk';

export interface AgentStreamRequest {
  prompt: string;
  provider?: AgentProvider;
  /** Stable id so the bridge reuses one ACP session across turns. */
  clientSessionId?: string;
  /** Optional extra context blocks the bridge prepends to the prompt. */
  context?: string;
}

export interface AgentStreamHandlers {
  /** Called with each incremental text fragment extracted from the stream. */
  onText: (delta: string) => void;
  /** Called once when the stream ends cleanly. */
  onDone?: () => void;
  /** Called on transport / provider error (the stream stops after this). */
  onError?: (message: string) => void;
}

/** Pull any human-readable text out of one normalized bridge stream item.
 *
 *  Both providers normalize to agent-devtools domain envelopes. Assistant text
 *  arrives as `{ type: 'acp.session_update', update: { sessionUpdate:
 *  'agent_message_chunk', content: { type: 'text', text } } }`. We also accept a
 *  few tolerant fallbacks (bare `text` / `delta`) so a provider tweak upstream
 *  doesn't silently blank the chat panel. Tool-call / result / system frames
 *  carry no assistant prose and are skipped (return ''). */
function extractText(item: unknown): string {
  if (typeof item === 'string') return item;
  if (!item || typeof item !== 'object') return '';
  const o = item as Record<string, unknown>;

  // Normalized session-update envelope (acp + sdk both emit this).
  const update = o.update as Record<string, unknown> | undefined;
  if (update && update.sessionUpdate === 'agent_message_chunk') {
    const content = update.content as Record<string, unknown> | undefined;
    if (content && content.type === 'text' && typeof content.text === 'string') {
      return content.text;
    }
  }

  // Tolerant fallbacks.
  if (typeof o.text === 'string') return o.text;
  if (typeof o.delta === 'string') return o.delta;
  return '';
}

/** Detect an error frame and return its message, or null when the item is not
 *  an error. The bridge emits `{ kind: 'error', error: { name, message } }` and
 *  the SDK path emits `{ type: 'acp.error', error: {...} }`. */
function extractError(item: unknown): string | null {
  if (!item || typeof item !== 'object') return null;
  const o = item as Record<string, unknown>;
  const isError =
    o.kind === 'error' || o.type === 'acp.error' || o.type === 'error';
  if (!isError) return null;
  const err = o.error;
  if (err && typeof err === 'object') {
    const e = err as Record<string, unknown>;
    const name = typeof e.name === 'string' ? e.name : 'error';
    const message = typeof e.message === 'string' ? e.message : '';
    return message ? `${name}: ${message}` : name;
  }
  return 'agent stream error';
}

/** Open a conversational agent stream. Returns an `abort()` that cancels the
 *  in-flight request (the caller wires it to component unmount / a stop button).
 *
 *  Parses SSE frames line-by-line: each event is delimited by a blank line and
 *  carries one or more `data:` lines whose concatenation is a JSON item. */
export function streamAgent(
  req: AgentStreamRequest,
  handlers: AgentStreamHandlers,
): { abort: () => void } {
  const controller = new AbortController();

  (async () => {
    let res: Response;
    try {
      res = await fetch(daemonUrl('/v1/agent/stream'), {
        method: 'POST',
        credentials: 'include',
        signal: controller.signal,
        headers: {
          'content-type': 'application/json',
          accept: 'text/event-stream',
          ...authHeaders(),
        },
        body: JSON.stringify({
          prompt: req.prompt,
          provider: req.provider ?? 'acp',
          ...(req.clientSessionId ? { clientSessionId: req.clientSessionId } : {}),
          ...(req.context ? { context: req.context } : {}),
        }),
      });
    } catch (err) {
      if (controller.signal.aborted) return;
      handlers.onError?.(
        err instanceof Error ? err.message : 'llm-bridge unreachable',
      );
      return;
    }

    if (!res.ok || !res.body) {
      // The proxy surfaces bridge failures as the daemon's JSON error contract.
      let message = `${res.status} ${res.statusText}`;
      try {
        const body = (await res.json()) as {
          error?: { code?: string; message?: string };
        };
        if (body?.error?.message) {
          message = `${body.error.code ?? 'ERROR'}: ${body.error.message}`;
        }
      } catch {
        /* non-JSON body — keep the status line */
      }
      handlers.onError?.(message);
      return;
    }

    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let buffer = '';

    const handleFrame = (frame: string) => {
      // Collect the `data:` lines of one SSE event; ignore id:/event:/comments.
      const dataLines = frame
        .split('\n')
        .filter((l) => l.startsWith('data:'))
        .map((l) => l.slice(5).replace(/^ /, ''));
      if (dataLines.length === 0) return;
      const payload = dataLines.join('\n');
      if (!payload || payload === '[DONE]') return;
      let item: unknown;
      try {
        item = JSON.parse(payload);
      } catch {
        // A provider that streams raw text rather than JSON — surface verbatim.
        handlers.onText(payload);
        return;
      }
      const errMsg = extractError(item);
      if (errMsg) {
        handlers.onError?.(errMsg);
        return;
      }
      const text = extractText(item);
      if (text) handlers.onText(text);
    };

    try {
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });
        // SSE events are separated by a blank line (\n\n, tolerate \r\n\r\n).
        let sep = buffer.search(/\r?\n\r?\n/);
        while (sep !== -1) {
          const frame = buffer.slice(0, sep);
          buffer = buffer.slice(sep + buffer.match(/\r?\n\r?\n/)![0].length);
          handleFrame(frame);
          sep = buffer.search(/\r?\n\r?\n/);
        }
      }
      if (buffer.trim()) handleFrame(buffer);
      handlers.onDone?.();
    } catch (err) {
      if (controller.signal.aborted) return;
      handlers.onError?.(
        err instanceof Error ? err.message : 'stream interrupted',
      );
    }
  })();

  return { abort: () => controller.abort() };
}
