import type { HTMLAttributes, Ref } from 'react';
import { cn } from '../../../lib/cn';

type WithRef<T extends HTMLElement> = HTMLAttributes<T> & { ref?: Ref<T> };

export function Root({ className, ref, ...rest }: WithRef<HTMLDivElement>) {
  return (
    <div
      ref={ref}
      data-slot="app-shell-root"
      className={cn(
        'flex h-screen w-screen overflow-hidden',
        'bg-background text-foreground font-sans',
        className,
      )}
      {...rest}
    />
  );
}

export function Sidebar({ className, ref, ...rest }: WithRef<HTMLElement>) {
  return (
    <aside
      ref={ref}
      data-slot="app-shell-sidebar"
      className={cn(
        'w-72 shrink-0',
        'border-r border-border bg-surface',
        'flex flex-col overflow-y-auto',
        className,
      )}
      {...rest}
    />
  );
}

export function Content({ className, ref, ...rest }: WithRef<HTMLDivElement>) {
  return (
    <div
      ref={ref}
      data-slot="app-shell-content"
      className={cn(
        'flex min-w-0 flex-1 flex-col',
        'bg-background',
        className,
      )}
      {...rest}
    />
  );
}

export function Topbar({ className, ref, ...rest }: WithRef<HTMLElement>) {
  return (
    <header
      ref={ref}
      data-slot="app-shell-topbar"
      className={cn(
        'h-12 shrink-0',
        'border-b border-border bg-surface',
        'flex items-center gap-2 px-4',
        className,
      )}
      {...rest}
    />
  );
}

export function Main({ className, ref, ...rest }: WithRef<HTMLElement>) {
  return (
    <main
      ref={ref}
      data-slot="app-shell-main"
      className={cn(
        'min-h-0 flex-1 overflow-auto',
        'bg-background',
        className,
      )}
      {...rest}
    />
  );
}

export type AppShellRootProps = WithRef<HTMLDivElement>;
export type AppShellSidebarProps = WithRef<HTMLElement>;
export type AppShellContentProps = WithRef<HTMLDivElement>;
export type AppShellTopbarProps = WithRef<HTMLElement>;
export type AppShellMainProps = WithRef<HTMLElement>;
