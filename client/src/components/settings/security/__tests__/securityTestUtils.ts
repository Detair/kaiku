import { setAuthState } from "@/stores/auth";
import type { MeSecurity, User } from "@/lib/types";

export function signInAs(assurance: "full" | "probation", extra: Partial<User> = {}): void {
  setAuthState({
    user: {
      id: "me",
      username: "alice",
      display_name: "Alice",
      avatar_url: null,
      status: "online",
      email: null,
      mfa_enabled: false,
      created_at: "",
      session: { id: "s-me", assurance, auth_methods: [], auth_time: "", client_class: "web", absolute_expires_at: "", key_device_id: null },
      ...extra,
    } as User,
  });
}

export function security(over: Partial<MeSecurity> = {}): MeSecurity {
  return {
    session: { assurance: "full", auth_methods: ["passkey"] },
    password: { state: "active" },
    totp: null,
    recovery_codes_left: 8,
    key_devices: 1,
    passkeys: 1,
    may_approve: true,
    bootstrap_open: false,
    recovery_contact: null,
    open_request: null,
    prompts: [],
    ...over,
  };
}
