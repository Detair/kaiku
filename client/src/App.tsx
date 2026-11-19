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
// Registration is not on the startup path: its own chunk.
const Register = lazy(() => import("./views/Register"));
import Main from "./views/Main";

// Views (lazy: not part of main flow)
const NotFound = lazy(() => import("./views/NotFound"));
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

// Components
import AuthGuard from "./components/auth/AuthGuard";
import AcceptanceManager from "./components/pages/AcceptanceManager";
import { ToastContainer } from "./components/ui/Toast";
import { ContextMenuContainer } from "./components/ui/ContextMenu";
import E2EESetupPrompt from "./components/E2EESetupPrompt";
import { isE2EEEnabled } from "./lib/featureFlags";
import { PageFallback, LazyErrorBoundary } from "./components/ui/LazyFallback";
import LazyDialog from "./components/ui/LazyDialog";
import { shouldShowOnboarding } from "./lib/onboardingGate";
import { authState } from "./stores/auth";
import SessionExpiredModal from "./components/auth/SessionExpiredModal";
// Full-session, step-up and sign-out-others dialogs: a lazy chunk with a
// visible failure (SessionDialogsSlot). Their state stays eager: a refusal
// reported before the chunk has loaded still opens the full-session dialog
// (Fable r2 R2).
import SessionDialogsSlot from "./components/auth/SessionDialogsSlot";
import "./lib/fullSessionRequired";
import type { ReportTarget } from "./components/modals/ReportModal";

// Windows that open on demand or only on a first run: each loads when it
// first shows, so none of them weighs on the startup payload.
const SetupWizard = lazy(() => import("./components/SetupWizard"));
const OnboardingWizard = lazy(() => import("./components/OnboardingWizard"));
const BlockConfirmModal = lazy(
  () => import("./components/modals/BlockConfirmModal"),
);
const ReportModal = lazy(() => import("./components/modals/ReportModal"));

// Context menu callbacks
import { onShowBlockConfirm, onShowReport } from "./lib/contextMenuBuilders";

import { fetchUploadLimits } from "./lib/tauri";
import { initDrafts } from "./stores/drafts";
import { initTrayBadge } from "./lib/trayBadge";
import { initUpdater } from "./lib/updater";
import { initTrayVoiceSync } from "./stores/voice";
import SkipLink from "./components/layout/SkipLink";

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
  onMount(() => {
    initDrafts();
    // Sync total unread count to the system tray badge (desktop only)
    initTrayBadge();
    // Tray shows the call ("Leave voice", "Mute") and warns on close (desktop only)
    initTrayVoiceSync();
    // Check for app updates shortly after startup (desktop only)
    initUpdater();
    // Fetch upload size limits from server (non-blocking)
    fetchUploadLimits().catch((err) =>
      console.warn("[App] Failed to fetch upload limits:", err),
    );
  });

  return (
    <div class="h-dvh bg-background-tertiary text-text-primary">
      <SkipLink />
      <div>
        {props.children}
      </div>
      <ToastContainer />
      <SessionExpiredModal />
      <SessionDialogsSlot />
      <ContextMenuContainer />

      <Show when={blockTarget()}>
        {(target) => (
          <LazyDialog name="BlockConfirmModal">
            <BlockConfirmModal
              userId={target().id}
              username={target().username}
              displayName={target().display_name}
              onClose={() => setBlockTarget(null)}
            />
          </LazyDialog>
        )}
      </Show>

      <Show when={reportTarget()}>
        {(target) => (
          <LazyDialog name="ReportModal">
            <ReportModal
              target={target()}
              onClose={() => setReportTarget(null)}
            />
          </LazyDialog>
        )}
      </Show>
    </div>
  );
};

// Protected route wrapper
const ProtectedMain: Component = () => (
  <AuthGuard>
    <Show when={authState.setupRequired}>
      <LazyDialog name="SetupWizard">
        <SetupWizard />
      </LazyDialog>
    </Show>
    <Show when={isE2EEEnabled()}>
      <E2EESetupPrompt />
    </Show>
    <Show when={shouldShowOnboarding()}>
      <LazyDialog name="OnboardingWizard">
        <OnboardingWizard />
      </LazyDialog>
    </Show>
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
const SetupLink = lazy(() => import("./views/SetupLink"));
const SetupLinkPage = () => (
  <Layout>
    <LazyErrorBoundary name="SetupLink">
      <Suspense fallback={<PageFallback />}>
        <SetupLink />
      </Suspense>
    </LazyErrorBoundary>
  </Layout>
);
const RecoverAccount = lazy(() => import("./views/RecoverAccount"));
const SecureAccount = lazy(() => import("./views/SecureAccount"));
const AccountRequestView = lazy(() => import("./views/AccountRequestView"));
const lazyPage = (name: string, View: Component) => () => (
  <Layout>
    <LazyErrorBoundary name={name}>
      <Suspense fallback={<PageFallback />}>
        <View />
      </Suspense>
    </LazyErrorBoundary>
  </Layout>
);
const RegistrationBind = lazy(() => import("./views/RegistrationBind"));
const RecoverPage = lazyPage("RecoverAccount", RecoverAccount);
const RegistrationBindPage = lazyPage("RegistrationBind", RegistrationBind);
const SecureAccountPage = lazyPage("SecureAccount", SecureAccount);
const AccountRequestPage = lazyPage("AccountRequest", AccountRequestView);
const RegisterPage = lazyPage("Register", Register);
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

const NotFoundPage = () => (
  <Layout>
    <LazyErrorBoundary name="NotFound">
      <Suspense fallback={<PageFallback />}>
        <NotFound />
      </Suspense>
    </LazyErrorBoundary>
  </Layout>
);

// Export routes as JSX Route elements
export const AppRoutes = (): JSX.Element => (
  <>
    {import.meta.env.DEV && <Route path="/demo" component={ThemeDemoPage} />}
    <Route path="/login" component={LoginPage} />
    <Route path="/register" component={RegisterPage} />
    <Route path="/register/bind" component={RegistrationBindPage} />
    <Route path="/setup" component={SetupLinkPage} />
    <Route path="/recover" component={RecoverPage} />
    <Route path="/secure-account" component={SecureAccountPage} />
    <Route path="/account-request" component={AccountRequestPage} />
    <Route path="/invite/:code" component={InvitePage} />
    <Route path="/pages/:slug" component={PagePage} />
    <Route path="/guilds/:guildId/pages/:slug" component={PagePage} />
    <Route path="/guilds/:guildId/library" component={MainPage} />
    <Route path="/admin" component={AdminPage} />
    <Route path="/settings/connection" component={ConnectionHistoryPage} />
    <Route path="/settings/bots/:id/commands" component={BotCommandsPage} />
    <Route path="/settings/bots/:id/webhooks" component={BotWebhooksPage} />
    <Route path="/404" component={NotFoundPage} />
    <Route path="/*" component={MainPage} />
  </>
);

export default AppRoutes;
