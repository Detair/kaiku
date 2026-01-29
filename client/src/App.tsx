import { Component, ParentProps, JSX, onMount } from "solid-js";
import { Route } from "@solidjs/router";

// Views
import Login from "./views/Login";
import Register from "./views/Register";
import Main from "./views/Main";
import ThemeDemo from "./pages/ThemeDemo";
import InviteJoin from "./views/InviteJoin";
import PageViewRoute from "./views/PageViewRoute";
import AdminDashboard from "./views/AdminDashboard";
import { ConnectionHistory } from "./pages/settings/ConnectionHistory";

// Components
import AuthGuard from "./components/auth/AuthGuard";
import { AcceptanceManager } from "./components/pages";
import { ToastContainer } from "./components/ui/Toast";
import { ContextMenuContainer } from "./components/ui/ContextMenu";
import E2EESetupPrompt from "./components/E2EESetupPrompt";
import SetupWizard from "./components/SetupWizard";

// Theme
import { initTheme } from "./stores/theme";

// Layout wrapper
const Layout: Component<ParentProps> = (props) => {
  onMount(async () => {
    await initTheme();
  });

  return (
    <div class="h-screen bg-background-tertiary text-text-primary">
      {props.children}
      <ToastContainer />
      <ContextMenuContainer />
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

// Wrapped components for routes
const LoginPage = () => <Layout><Login /></Layout>;
const RegisterPage = () => <Layout><Register /></Layout>;
const MainPage = () => <Layout><ProtectedMain /></Layout>;
const ThemeDemoPage = () => <Layout><ThemeDemo /></Layout>;
const InvitePage = () => <Layout><ProtectedInvite /></Layout>;
const PagePage = () => <Layout><ProtectedPageView /></Layout>;
const AdminPage = () => <Layout><ProtectedAdmin /></Layout>;
const ConnectionHistoryPage = () => <Layout><ProtectedConnectionHistory /></Layout>;

// Export routes as JSX Route elements
export const AppRoutes = (): JSX.Element => (
  <>
    <Route path="/demo" component={ThemeDemoPage} />
    <Route path="/login" component={LoginPage} />
    <Route path="/register" component={RegisterPage} />
    <Route path="/invite/:code" component={InvitePage} />
    <Route path="/pages/:slug" component={PagePage} />
    <Route path="/guilds/:guildId/pages/:slug" component={PagePage} />
    <Route path="/admin" component={AdminPage} />
    <Route path="/settings/connection" component={ConnectionHistoryPage} />
    <Route path="/*" component={MainPage} />
  </>
);

export default AppRoutes;
