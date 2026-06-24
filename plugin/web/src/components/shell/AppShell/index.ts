import { Root, Sidebar, Content, Topbar, Main } from './parts';

/**
 * AppShell — top-level desktop chrome compound.
 *
 *   <AppShell.Root>
 *     <AppShell.Sidebar>…</AppShell.Sidebar>
 *     <AppShell.Content>
 *       <AppShell.Topbar>…</AppShell.Topbar>
 *       <AppShell.Main>…</AppShell.Main>
 *     </AppShell.Content>
 *   </AppShell.Root>
 *
 * Root owns the full viewport, Sidebar is fixed-width and scrolls
 * independently, Content stacks Topbar above Main with Main owning vertical
 * overflow. No internal state — pure layout composition. React 19 accepts ref
 * as a regular prop, so forwardRef is unnecessary.
 */
export const AppShell = {
  Root,
  Sidebar,
  Content,
  Topbar,
  Main,
};

export type {
  AppShellRootProps,
  AppShellSidebarProps,
  AppShellContentProps,
  AppShellTopbarProps,
  AppShellMainProps,
} from './parts';
