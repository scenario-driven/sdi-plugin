import { useEffect, useRef, useState } from 'react';
import type { DecisionQuestion, QuestionOption } from '../types/entities';
import { streamAgent } from '../lib/agentChat';
import { Button } from './ui';
import { Markdown } from './Markdown';
import { cn } from '../lib/cn';

interface QuestionChatProps {
  question: DecisionQuestion;
  options: QuestionOption[];
}

interface ChatTurn {
  role: 'user' | 'assistant';
  text: string;
  /** True while the assistant turn is still streaming. */
  streaming?: boolean;
}

/** D36 conversational mode — discuss one decision question with the LLM
 *  (subscription via the llm-bridge) "as if confirming the answer" before
 *  committing. Stateful ACP session keyed by the question id so successive
 *  turns share context. This panel is a *discussion aid*; the actual answer is
 *  still submitted through the option buttons / free-text on the card. */
export function QuestionChat({ question, options }: QuestionChatProps) {
  const [turns, setTurns] = useState<ChatTurn[]>([]);
  const [draft, setDraft] = useState('');
  const [busy, setBusy] = useState(false);
  const abortRef = useRef<(() => void) | null>(null);
  const scrollRef = useRef<HTMLDivElement | null>(null);

  // Abort an in-flight stream if the component unmounts.
  useEffect(() => () => abortRef.current?.(), []);

  // Keep the transcript pinned to the latest line.
  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight });
  }, [turns]);

  const send = (prompt: string) => {
    if (!prompt.trim() || busy) return;
    setBusy(true);
    setTurns((prev) => [
      ...prev,
      { role: 'user', text: prompt },
      { role: 'assistant', text: '', streaming: true },
    ]);

    const appendToLast = (delta: string) =>
      setTurns((prev) => {
        const next = [...prev];
        const last = next[next.length - 1];
        if (last && last.role === 'assistant') {
          next[next.length - 1] = { ...last, text: last.text + delta };
        }
        return next;
      });

    const finishLast = (errText?: string) =>
      setTurns((prev) => {
        const next = [...prev];
        const last = next[next.length - 1];
        if (last && last.role === 'assistant') {
          next[next.length - 1] = {
            ...last,
            streaming: false,
            text: errText
              ? `${last.text}${last.text ? '\n\n' : ''}⚠ ${errText}`
              : last.text || '_(no response)_',
          };
        }
        return next;
      });

    const { abort } = streamAgent(
      {
        prompt,
        provider: 'acp',
        clientSessionId: `dq-${question.id}`,
      },
      {
        onText: appendToLast,
        onDone: () => {
          finishLast();
          setBusy(false);
        },
        onError: (msg) => {
          finishLast(msg);
          setBusy(false);
        },
      },
    );
    abortRef.current = abort;
  };

  // Seed prompt — hand the LLM the stem + options so it can argue each choice
  // "exam-review" style (D35: 정답 확인하듯).
  const startReview = () => {
    const optionsBlock = options
      .map(
        (o, i) =>
          `${i + 1}. ${o.label}${o.is_llm_recommended ? ' (LLM 권장)' : ''}` +
          (o.rationale_md ? `\n   해설: ${o.rationale_md}` : ''),
      )
      .join('\n');
    const prompt =
      `다음은 SDI oracle 결정-질문(${question.qtype})입니다. ` +
      `각 보기를 "정답 확인하듯" 비교하고, 어떤 선택이 더 맞고 그른지 근거와 함께 토론해 주세요.\n\n` +
      `## 맥락\n${question.context_md}\n\n## 보기\n${optionsBlock}`;
    send(prompt);
  };

  return (
    <div className="rounded-md border border-border bg-background p-3 space-y-3">
      <div className="flex items-center justify-between gap-2">
        <span className="text-[10px] uppercase tracking-wide text-muted">
          대화 모드 — LLM 토론 (구독)
        </span>
        {turns.length === 0 && (
          <Button size="sm" variant="secondary" onClick={startReview} disabled={busy}>
            정답 확인 토론 시작
          </Button>
        )}
      </div>

      {turns.length > 0 && (
        <div
          ref={scrollRef}
          className="max-h-72 overflow-auto space-y-3 rounded border border-border bg-surface p-2"
        >
          {turns.map((t, i) => (
            <div
              key={i}
              className={cn(
                'rounded-md px-2.5 py-1.5 text-sm',
                t.role === 'user'
                  ? 'bg-primary/10 text-foreground'
                  : 'bg-surface-high/60 text-foreground',
              )}
            >
              <div className="text-[10px] uppercase tracking-wide text-muted mb-1">
                {t.role === 'user' ? 'you' : 'llm'}
              </div>
              {t.text ? (
                <Markdown className="prose-p:my-1">{t.text}</Markdown>
              ) : (
                <span className="text-muted">…</span>
              )}
              {t.streaming && (
                <span className="ml-0.5 inline-block animate-pulse text-muted">▍</span>
              )}
            </div>
          ))}
        </div>
      )}

      <form
        className="flex items-end gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          const text = draft;
          setDraft('');
          send(text);
        }}
      >
        <textarea
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              const text = draft;
              setDraft('');
              send(text);
            }
          }}
          rows={2}
          placeholder="보기에 대해 더 묻거나 반론하기… (⌘/Ctrl+Enter)"
          className={cn(
            'flex-1 resize-none rounded-md border border-border bg-surface',
            'px-2.5 py-1.5 text-sm text-foreground placeholder:text-muted',
            'focus:outline-none focus:ring-2 focus:ring-ring',
          )}
        />
        <Button type="submit" size="sm" disabled={busy || !draft.trim()}>
          보내기
        </Button>
      </form>
    </div>
  );
}
