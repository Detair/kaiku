/** The frame of the public sign-in help pages (/recover, /secure-account, /account-request). */

import { Component, JSX } from "solid-js";

const AuthPageShell: Component<{ title: string; children: JSX.Element }> = (props) => (
  <div class="min-h-dvh overflow-y-auto flex items-start sm:items-center justify-center bg-background-primary p-4">
    <main class="w-full max-w-lg bg-background-secondary rounded-xl shadow-lg p-6 space-y-5">
      <h1 class="text-2xl font-bold text-text-primary">{props.title}</h1>
      {props.children}
    </main>
  </div>
);

export default AuthPageShell;

export const fieldLabel = "block text-base font-medium text-text-primary mb-1";
export const fieldInput = "input-field min-h-12 text-base";
export const errorStyle = {
  "background-color": "var(--color-error-bg)",
  border: "1px solid var(--color-error-border)",
  color: "var(--color-error-text)",
} as const;
