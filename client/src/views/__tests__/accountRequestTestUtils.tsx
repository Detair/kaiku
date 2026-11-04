import { Router, Route } from "@solidjs/router";
import type { Component } from "solid-js";
import type { AccountRequest } from "@/lib/types";

export function requestOf(over: Partial<AccountRequest> = {}): AccountRequest {
  return {
    id: "r1",
    kind: "self",
    purpose: "recovery",
    state: "waiting",
    created_at: "2026-11-03T08:00:00Z",
    ready_at: "2026-11-04T08:00:00Z",
    expires_at: null,
    factors: ["recovery_code", "password"],
    lost_key_devices: 1,
    staged: { authenticator: null, password: false, totp: false, recovery_contact: null },
    ...over,
  };
}

export function routed(View: Component, path: string) {
  return () => (
    <Router>
      <Route path={path} component={View} />
      <Route path="*" component={() => <div data-testid="elsewhere" />} />
    </Router>
  );
}
