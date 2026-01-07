import { Component } from "solid-js";
import { Routes, Route } from "@solidjs/router";

// Views
import Login from "./views/Login";
import Register from "./views/Register";
import Main from "./views/Main";

// Components
import AuthGuard from "./components/auth/AuthGuard";

const App: Component = () => {
  return (
    <div class="h-screen bg-background-tertiary text-text-primary">
      <Routes>
        {/* Public routes */}
        <Route path="/login" component={Login} />
        <Route path="/register" component={Register} />

        {/* Protected routes */}
        <Route
          path="/*"
          component={() => (
            <AuthGuard>
              <Main />
            </AuthGuard>
          )}
        />
      </Routes>
    </div>
  );
};

export default App;
