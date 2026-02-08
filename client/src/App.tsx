import { Component, ParentProps, JSX, onMount, createSignal, Show } from "solid-js";
import { Route } from "@solidjs/router";

// Views
import Login from "./views/Login";
import Register from "./views/Register";
import ForgotPassword from "./views/ForgotPassword";
import ResetPassword from "./views/ResetPassword";
import Main from "./views/Main";
import ThemeDemo from "./pages/ThemeDemo";
import InviteJoin from "./views/InviteJoin";
import PageViewRoute from "./views/PageViewRoute";
import AdminDashboard from "./views/AdminDashboard";
import { ConnectionHistory } from "./pages/settings/ConnectionHistory";
import BotSlashCommands from "./pages/settings/BotSlashCommands";

// Components
import AuthGuard from "./components/auth/AuthGuard";
import { AcceptanceManager } from "./components/pages";
import { ToastContainer } from "./components/ui/Toast";
import { ContextMenuContainer } from "./components/ui/ContextMenu";
import E2EESetupPrompt from "./components/E2EESetupPrompt";
import SetupWizard from "./components/SetupWizard";
import BlockConfirmModal from "./components/modals/BlockConfirmModal";
import ReportModal from "./components/modals/ReportModal";
import type { ReportTarget } from "./components/modals/ReportModal";

// Context menu callbacks
import { onShowBlockConfirm, onShowReport } from "./lib/contextMenuBuilders";

// Theme
import { initTheme } from "./stores/theme";
import { fetchUploadLimits } from "./lib/tauri";
import { initDrafts } from "./stores/drafts";

// Global modal state
const [blockTarget, setBlockTarget] = createSignal<{ id: string; username: string; display_name?: string } | null>(null);
const [reportTarget, setReportTarget] = createSignal<ReportTarget | null>(null);

// Register context menu callbacks
onShowBlockConfirm((target) => setBlockTarget({ id: target.id, username: target.username, display_name: target.display_name }));
onShowReport((target) => setReportTarget({ userId: target.userId, username: target.username, messageId: target.messageId }));

// Layout wrapper
const Layout: Component<ParentProps> = (props) => {
  onMount(async () => {
    await initTheme();
    initDrafts();
    // Fetch upload size limits from server (non-blocking)
    fetchUploadLimits().catch(err =>
      console.warn('[App] Failed to fetch upload limits:', err)
    );
  });

  return (
    <div class="h-screen bg-background-tertiary text-text-primary">
      {props.children}
      <ToastContainer />
      <ContextMenuContainer />

      <Show when={blockTarget()}>
        {(target) => (
          <BlockConfirmModal
            userId={target().id}
            username={target().username}
            displayName={target().display_name}
            onClose={() => setBlockTarget(null)}
          />
        )}
      </Show>

      <Show when={reportTarget()}>
        {(target) => (
          <ReportModal
            target={target()}
            onClose={() => setReportTarget(null)}
          />
        )}
      </Show>
    </div>
  );
};

// Protected route wrapper
const ProtectedMain: Component = () => (
  <AuthGuard>
    <SetupWizard />
    <E2EESetupPrompt />
    <AcceptanceManager />
    <Main />
  </AuthGuard>
);

// Protected invite wrapper (needs auth check but shows loading state)
const ProtectedInvite: Component = () => (
  <AuthGuard>
    <InviteJoin />
  </AuthGuard>
);

// Protected page view wrapper
const ProtectedPageView: Component = () => (
  <AuthGuard>
    <PageViewRoute />
  </AuthGuard>
);

// Protected admin wrapper
const ProtectedAdmin: Component = () => (
  <AuthGuard>
    <AdminDashboard />
  </AuthGuard>
);

// Protected connection history wrapper
const ProtectedConnectionHistory: Component = () => (
  <AuthGuard>
    <ConnectionHistory />
  </AuthGuard>
);

// Protected bot commands wrapper
const ProtectedBotCommands: Component = () => (
  <AuthGuard>
    <BotSlashCommands />
  </AuthGuard>
);

// Wrapped components for routes
const LoginPage = () => <Layout><Login /></Layout>;
const RegisterPage = () => <Layout><Register /></Layout>;
const ForgotPasswordPage = () => <Layout><ForgotPassword /></Layout>;
const ResetPasswordPage = () => <Layout><ResetPassword /></Layout>;
const MainPage = () => <Layout><ProtectedMain /></Layout>;
const ThemeDemoPage = () => <Layout><ThemeDemo /></Layout>;
const InvitePage = () => <Layout><ProtectedInvite /></Layout>;
const PagePage = () => <Layout><ProtectedPageView /></Layout>;
const AdminPage = () => <Layout><ProtectedAdmin /></Layout>;
const ConnectionHistoryPage = () => <Layout><ProtectedConnectionHistory /></Layout>;
const BotCommandsPage = () => <Layout><ProtectedBotCommands /></Layout>;

// Export routes as JSX Route elements
export const AppRoutes = (): JSX.Element => (
  <>
    <Route path="/demo" component={ThemeDemoPage} />
    <Route path="/login" component={LoginPage} />
    <Route path="/register" component={RegisterPage} />
    <Route path="/forgot-password" component={ForgotPasswordPage} />
    <Route path="/reset-password" component={ResetPasswordPage} />
    <Route path="/invite/:code" component={InvitePage} />
    <Route path="/pages/:slug" component={PagePage} />
    <Route path="/guilds/:guildId/pages/:slug" component={PagePage} />
    <Route path="/admin" component={AdminPage} />
    <Route path="/settings/connection" component={ConnectionHistoryPage} />
    <Route path="/settings/bots/:id/commands" component={BotCommandsPage} />
    <Route path="/*" component={MainPage} />
  </>
);

export default AppRoutes;
