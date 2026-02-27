import {
  Component,
  ParentProps,
  JSX,
  onMount,
  createSignal,
  Show,
  lazy,
  Suspense,
} from "solid-js";
import { Route } from "@solidjs/router";

// Views (eager: first views users see)
import Login from "./views/Login";
import Register from "./views/Register";
import Main from "./views/Main";

// Views (lazy: not part of main flow)
const ForgotPassword = lazy(() => import("./views/ForgotPassword"));
const ResetPassword = lazy(() => import("./views/ResetPassword"));
const ThemeDemo = lazy(() => import("./pages/ThemeDemo"));
const InviteJoin = lazy(() => import("./views/InviteJoin"));
const PageViewRoute = lazy(() => import("./views/PageViewRoute"));
const AdminDashboard = lazy(() => import("./views/AdminDashboard"));
const ConnectionHistory = lazy(
  () => import("./pages/settings/ConnectionHistory"),
);
const BotSlashCommands = lazy(
  () => import("./pages/settings/BotSlashCommands"),
);
const BotWebhooks = lazy(() => import("./pages/settings/BotWebhooks"));
const LibraryViewRoute = lazy(() => import("./views/LibraryViewRoute"));

// Components
import AuthGuard from "./components/auth/AuthGuard";
import AcceptanceManager from "./components/pages/AcceptanceManager";
import { ToastContainer } from "./components/ui/Toast";
import { ContextMenuContainer } from "./components/ui/ContextMenu";
import E2EESetupPrompt from "./components/E2EESetupPrompt";
import { PageFallback, LazyErrorBoundary } from "./components/ui/LazyFallback";
import SetupWizard from "./components/SetupWizard";
import OnboardingWizard from "./components/OnboardingWizard";
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
const [blockTarget, setBlockTarget] = createSignal<{
  id: string;
  username: string;
  display_name?: string;
} | null>(null);
const [reportTarget, setReportTarget] = createSignal<ReportTarget | null>(null);

// Register context menu callbacks
onShowBlockConfirm((target) =>
  setBlockTarget({
    id: target.id,
    username: target.username,
    display_name: target.display_name,
  }),
);
onShowReport((target) =>
  setReportTarget({
    userId: target.userId,
    username: target.username,
    messageId: target.messageId,
  }),
);

// Layout wrapper
const Layout: Component<ParentProps> = (props) => {
  onMount(async () => {
    await initTheme();
    initDrafts();
    // Fetch upload size limits from server (non-blocking)
    fetchUploadLimits().catch((err) =>
      console.warn("[App] Failed to fetch upload limits:", err),
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
    <OnboardingWizard />
    <AcceptanceManager />
    <Main />
  </AuthGuard>
);

// Protected invite wrapper (needs auth check but shows loading state)
const ProtectedInvite: Component = () => (
  <AuthGuard>
    <LazyErrorBoundary name="InviteJoin">
      <Suspense fallback={<PageFallback />}>
        <InviteJoin />
      </Suspense>
    </LazyErrorBoundary>
  </AuthGuard>
);

// Protected page view wrapper
const ProtectedPageView: Component = () => (
  <AuthGuard>
    <LazyErrorBoundary name="PageView">
      <Suspense fallback={<PageFallback />}>
        <PageViewRoute />
      </Suspense>
    </LazyErrorBoundary>
  </AuthGuard>
);

// Protected admin wrapper
const ProtectedAdmin: Component = () => (
  <AuthGuard>
    <LazyErrorBoundary name="AdminDashboard">
      <Suspense fallback={<PageFallback />}>
        <AdminDashboard />
      </Suspense>
    </LazyErrorBoundary>
  </AuthGuard>
);

// Protected connection history wrapper
const ProtectedConnectionHistory: Component = () => (
  <AuthGuard>
    <LazyErrorBoundary name="ConnectionHistory">
      <Suspense fallback={<PageFallback />}>
        <ConnectionHistory />
      </Suspense>
    </LazyErrorBoundary>
  </AuthGuard>
);

// Protected bot commands wrapper
const ProtectedBotCommands: Component = () => (
  <AuthGuard>
    <LazyErrorBoundary name="BotSlashCommands">
      <Suspense fallback={<PageFallback />}>
        <BotSlashCommands />
      </Suspense>
    </LazyErrorBoundary>
  </AuthGuard>
);

// Protected bot webhooks wrapper
const ProtectedBotWebhooks: Component = () => (
  <AuthGuard>
    <LazyErrorBoundary name="BotWebhooks">
      <Suspense fallback={<PageFallback />}>
        <BotWebhooks />
      </Suspense>
    </LazyErrorBoundary>
  </AuthGuard>
);

// Wrapped components for routes
const LoginPage = () => (
  <Layout>
    <Login />
  </Layout>
);
const RegisterPage = () => (
  <Layout>
    <Register />
  </Layout>
);
const ForgotPasswordPage = () => (
  <Layout>
    <LazyErrorBoundary name="ForgotPassword">
      <Suspense fallback={<PageFallback />}>
        <ForgotPassword />
      </Suspense>
    </LazyErrorBoundary>
  </Layout>
);
const ResetPasswordPage = () => (
  <Layout>
    <LazyErrorBoundary name="ResetPassword">
      <Suspense fallback={<PageFallback />}>
        <ResetPassword />
      </Suspense>
    </LazyErrorBoundary>
  </Layout>
);
const MainPage = () => (
  <Layout>
    <ProtectedMain />
  </Layout>
);
const ThemeDemoPage = () => (
  <Layout>
    <LazyErrorBoundary name="ThemeDemo">
      <Suspense fallback={<PageFallback />}>
        <ThemeDemo />
      </Suspense>
    </LazyErrorBoundary>
  </Layout>
);
const InvitePage = () => (
  <Layout>
    <ProtectedInvite />
  </Layout>
);
const PagePage = () => (
  <Layout>
    <ProtectedPageView />
  </Layout>
);
const AdminPage = () => (
  <Layout>
    <ProtectedAdmin />
  </Layout>
);
const ConnectionHistoryPage = () => (
  <Layout>
    <ProtectedConnectionHistory />
  </Layout>
);
const BotCommandsPage = () => (
  <Layout>
    <ProtectedBotCommands />
  </Layout>
);
const BotWebhooksPage = () => (
  <Layout>
    <ProtectedBotWebhooks />
  </Layout>
);

// Protected library wrapper
const ProtectedLibrary: Component = () => (
  <AuthGuard>
    <LazyErrorBoundary name="LibraryView">
      <Suspense fallback={<PageFallback />}>
        <LibraryViewRoute />
      </Suspense>
    </LazyErrorBoundary>
  </AuthGuard>
);
const LibraryPage = () => (
  <Layout>
    <ProtectedLibrary />
  </Layout>
);

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
    <Route path="/guilds/:guildId/library" component={LibraryPage} />
    <Route path="/admin" component={AdminPage} />
    <Route path="/settings/connection" component={ConnectionHistoryPage} />
    <Route path="/settings/bots/:id/commands" component={BotCommandsPage} />
    <Route path="/settings/bots/:id/webhooks" component={BotWebhooksPage} />
    <Route path="/*" component={MainPage} />
  </>
);

export default AppRoutes;
