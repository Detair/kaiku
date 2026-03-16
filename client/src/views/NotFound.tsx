import { Component } from "solid-js";
import { A } from "@solidjs/router";

const NotFound: Component = () => {
  document.title = "Page Not Found | Kaiku";

  return (
    <div class="flex items-center justify-center min-h-screen bg-background-primary">
      <div class="text-center p-8">
        <h1 class="text-6xl font-bold text-accent-primary mb-4">404</h1>
        <p class="text-xl text-text-primary mb-2">Page not found</p>
        <p class="text-text-secondary mb-6">
          The page you're looking for doesn't exist or has been moved.
        </p>
        <A href="/" class="btn-primary inline-flex items-center px-6 py-3">
          Go Home
        </A>
      </div>
    </div>
  );
};

export default NotFound;
