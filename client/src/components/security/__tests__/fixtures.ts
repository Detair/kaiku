import type { SecurityEvent, SecurityNotice } from "@/lib/types";

export function event(over: Partial<SecurityEvent> = {}): SecurityEvent {
  return {
    id: "e1",
    event_type: "sign_in",
    severity: "normal",
    created_at: "2026-11-03T10:00:00Z",
    subject_id: "me",
    actor: { kind: "self", user_id: "me", username: "alice" },
    client: { class: "desktop", label: "Kaiku desktop on Windows" },
    place: null,
    ip: null,
    session_id: "s9",
    authenticator: null,
    request_id: null,
    details: { auth_methods: ["qr"], assurance: "probation" },
    actions: [],
    repudiated_at: null,
    ...over,
  };
}

export function notice(over: Partial<SecurityNotice> = {}, events: SecurityEvent[] = [event()]): SecurityNotice {
  return {
    notice_id: "n1",
    subject: { user_id: "me", username: "alice", display_name: "Alice" },
    event_type: events[0]?.event_type ?? "sign_in",
    severity: "normal",
    count: events.length,
    first_at: "2026-11-03T10:00:00Z",
    last_at: "2026-11-03T10:00:00Z",
    events,
    events_truncated: false,
    ...over,
  };
}
