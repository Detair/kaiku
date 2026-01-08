import { Component, ParentProps, JSX } from "solid-js";
import { Route } from "@solidjs/router";

// Views
import Login from "./views/Login";
import Register from "./views/Register";
import Main from "./views/Main";

// Components
import AuthGuard from "./components/auth/AuthGuard";

// Layout wrapper
const Layout: Component<ParentProps> = (props) => {
  return (
    <div class="h-screen bg-background-tertiary text-text-primary">
      {props.children}
    </div>
  );
};

// Protected route wrapper
const ProtectedMain: Component = () => (
  <AuthGuard>
    <Main />
  </AuthGuard>
);

// Wrapped components for routes
const LoginPage = () => <Layout><Login /></Layout>;
const RegisterPage = () => <Layout><Register /></Layout>;
const MainPage = () => <Layout><ProtectedMain /></Layout>;

// Export routes as JSX Route elements
export const AppRoutes = (): JSX.Element => (
  <>
    <Route path="/login" component={LoginPage} />
    <Route path="/register" component={RegisterPage} />
    <Route path="/*" component={MainPage} />
  </>
);

export default AppRoutes;
