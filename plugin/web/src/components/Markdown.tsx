import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { cn } from '../lib/cn';

interface MarkdownProps {
  children: string;
  className?: string;
}

/** Shared markdown surface matching the dashboard's existing prose styling
 *  (see DecisionDetail / RequirementDetail). Centralizes the Tailwind `prose`
 *  class soup so question stems, option rationales, and chat replies render
 *  identically. */
export function Markdown({ children, className }: MarkdownProps) {
  return (
    <div
      className={cn(
        'prose prose-sm max-w-none text-foreground',
        'prose-headings:text-foreground prose-strong:text-foreground',
        'prose-code:text-foreground prose-a:text-primary',
        className,
      )}
    >
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{children}</ReactMarkdown>
    </div>
  );
}
