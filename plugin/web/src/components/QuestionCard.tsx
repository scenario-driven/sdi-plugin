import { useEffect, useState } from 'react';
import type { DecisionQuestion, QuestionOption } from '../types/entities';
import { answerQuestion, listQuestionOptions } from '../lib/api';
import type { AnswerQuestionBody } from '../lib/api';
import { Badge, Button } from './ui';
import { Markdown } from './Markdown';
import { QuestionChat } from './QuestionChat';
import { toastSuccess, toastError } from '../lib/toast';
import { cn } from '../lib/cn';

interface QuestionCardProps {
  question: DecisionQuestion;
  /** When set, the card defers submission to the parent (batch mode): instead
   *  of POSTing immediately, it reports the staged selection upward. */
  onStage?: (questionId: string, body: AnswerQuestionBody | null) => void;
  /** A selection staged by the parent in batch mode (option id or free text). */
  staged?: AnswerQuestionBody | null;
  /** Bumped by the parent to force an options re-fetch after an answer. */
  refreshKey?: number;
  /** Called after a successful immediate (conversational-mode) submit. */
  onAnswered?: () => void;
}

const QTYPE_META: Record<
  DecisionQuestion['qtype'],
  { badge: 'success' | 'warning'; label: string; blurb: string }
> = {
  fact: {
    badge: 'success',
    label: 'fact · 베스트프랙티스',
    blurb:
      '소거 후 1개 생존 — LLM 권장안으로 자동결정됩니다. 나머지 보기는 해설/투명성용입니다.',
  },
  preference: {
    badge: 'warning',
    label: 'preference · 트레이드오프',
    blurb:
      '정답 없음 — 보기는 동급 트레이드오프 카드입니다. 사용자가 결정합니다.',
  },
};

/** D35 SA-exam decision card. Renders the stem (`context_md`), the option list
 *  (label + rationale + LLM-recommended badge), a `+@` free-text choice, and a
 *  conversational-mode discussion panel. The fact/preference distinction is
 *  enforced visually: a fact question foregrounds the recommended answer (the
 *  others are shown as transparency), a preference question presents the
 *  options as peer cards. */
export function QuestionCard({
  question,
  onStage,
  staged,
  refreshKey,
  onAnswered,
}: QuestionCardProps) {
  const [options, setOptions] = useState<QuestionOption[] | null>(null);
  const loading = options === null;
  const [freeText, setFreeText] = useState('');
  const [showFree, setShowFree] = useState(false);
  const [showChat, setShowChat] = useState(false);
  const [submitting, setSubmitting] = useState(false);

  const batch = !!onStage;
  const answered = question.status !== 'open';
  const meta = QTYPE_META[question.qtype];

  useEffect(() => {
    let cancelled = false;
    listQuestionOptions(question.id)
      .then((opts) => {
        if (cancelled) return;
        setOptions([...opts].sort((a, b) => a.idx - b.idx));
      })
      .catch(() => {
        if (!cancelled) setOptions([]);
      });
    return () => {
      cancelled = true;
    };
  }, [question.id, refreshKey]);

  const optionList = options ?? [];
  const recommendedId =
    optionList.find((o) => o.is_llm_recommended)?.id ?? null;
  const stagedOptionId = staged?.chosen_option_id ?? null;
  const stagedFree = staged?.free_text ?? null;

  // Submit one answer immediately (conversational mode). `auto` is set for a
  // fact-type question so the daemon flips status to `auto_decided`.
  const submit = async (body: AnswerQuestionBody) => {
    setSubmitting(true);
    try {
      await answerQuestion(question.id, {
        ...body,
        auto: question.qtype === 'fact',
      });
      toastSuccess(`${question.short_code} 답변 기록됨`);
      onAnswered?.();
    } catch (err) {
      toastError(err instanceof Error ? err.message : '답변 실패');
    } finally {
      setSubmitting(false);
    }
  };

  const chooseOption = (optionId: string) => {
    if (answered) return;
    if (batch) {
      const already = stagedOptionId === optionId;
      onStage!(question.id, already ? null : { chosen_option_id: optionId });
    } else {
      void submit({ chosen_option_id: optionId });
    }
  };

  const submitFree = () => {
    if (answered || !freeText.trim()) return;
    if (batch) {
      onStage!(question.id, { free_text: freeText.trim() });
    } else {
      void submit({ free_text: freeText.trim() });
    }
  };

  return (
    <article
      data-qtype={question.qtype}
      className={cn(
        'rounded-lg border bg-surface p-4 space-y-3',
        answered ? 'border-border opacity-80' : 'border-border',
      )}
    >
      {/* Header — qtype distinction + status. */}
      <header className="flex flex-wrap items-center gap-2">
        <span className="text-[10px] font-mono text-muted">{question.short_code}</span>
        <Badge size="sm" variant={meta.badge}>
          {meta.label}
        </Badge>
        {question.parent_question_id && (
          <Badge size="sm" variant="info">
            적응형 분기
          </Badge>
        )}
        <span className="ml-auto">
          <Badge
            size="sm"
            variant={
              question.status === 'open'
                ? 'default'
                : question.status === 'auto_decided'
                  ? 'success'
                  : 'primary'
            }
          >
            {question.status}
          </Badge>
        </span>
      </header>

      <p className="text-[11px] text-muted">{meta.blurb}</p>

      {/* Stem. */}
      <div className="rounded-md border border-border bg-background p-3">
        <Markdown>{question.context_md}</Markdown>
      </div>

      {/* Options. */}
      {loading ? (
        <p className="text-sm text-muted">보기 로딩…</p>
      ) : (
        <ul className="space-y-2">
          {optionList.map((o) => {
            const isRecommended = o.is_llm_recommended;
            const isStaged = stagedOptionId === o.id;
            // Fact: the recommended option is foregrounded; others are dimmed
            // transparency entries. Preference: all peers.
            const dimmedFact =
              question.qtype === 'fact' && recommendedId !== null && !isRecommended;
            return (
              <li key={o.id}>
                <button
                  type="button"
                  disabled={answered || submitting}
                  onClick={() => chooseOption(o.id)}
                  data-recommended={isRecommended || undefined}
                  data-staged={isStaged || undefined}
                  className={cn(
                    'w-full text-left rounded-md border p-3 transition-colors',
                    'disabled:cursor-not-allowed',
                    isStaged
                      ? 'border-primary bg-primary/10'
                      : isRecommended
                        ? 'border-success/50 bg-success/5 hover:bg-success/10'
                        : 'border-border bg-background hover:bg-surface-hover',
                    dimmedFact && !isStaged && 'opacity-60',
                  )}
                >
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-medium text-foreground">
                      {o.label}
                    </span>
                    {isRecommended && (
                      <Badge size="sm" variant="success">
                        ★ LLM 권장
                      </Badge>
                    )}
                    {isStaged && (
                      <Badge size="sm" variant="primary">
                        선택됨
                      </Badge>
                    )}
                  </div>
                  {o.body_md && (
                    <div className="mt-1.5">
                      <Markdown className="prose-p:my-1">{o.body_md}</Markdown>
                    </div>
                  )}
                  {o.rationale_md && (
                    <div className="mt-2 rounded border-l-2 border-border pl-2">
                      <div className="text-[10px] uppercase tracking-wide text-muted">
                        해설
                      </div>
                      <Markdown className="prose-p:my-0.5 text-muted">
                        {o.rationale_md}
                      </Markdown>
                    </div>
                  )}
                </button>
              </li>
            );
          })}
        </ul>
      )}

      {/* +@ free-text + chat toggles. */}
      {!answered && (
        <div className="space-y-2">
          <div className="flex flex-wrap items-center gap-2">
            <Button
              size="sm"
              variant="outline"
              onClick={() => setShowFree((v) => !v)}
            >
              +@ 주관식
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => setShowChat((v) => !v)}
            >
              {showChat ? '대화 닫기' : '대화 모드'}
            </Button>
          </div>

          {showFree && (
            <div className="space-y-2">
              <textarea
                value={freeText}
                onChange={(e) => setFreeText(e.target.value)}
                rows={2}
                placeholder="보기에 없는 답을 직접 입력…"
                className={cn(
                  'w-full resize-none rounded-md border border-border bg-background',
                  'px-2.5 py-1.5 text-sm text-foreground placeholder:text-muted',
                  'focus:outline-none focus:ring-2 focus:ring-ring',
                )}
              />
              {stagedFree && (
                <p className="text-[11px] text-primary">
                  staged free-text: {stagedFree}
                </p>
              )}
              <Button
                size="sm"
                onClick={submitFree}
                disabled={submitting || !freeText.trim()}
              >
                {batch ? '+@ 스테이지' : '+@ 제출'}
              </Button>
            </div>
          )}

          {showChat && <QuestionChat question={question} options={optionList} />}
        </div>
      )}
    </article>
  );
}
