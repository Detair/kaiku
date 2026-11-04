// Contract amendment 2026-11-03 (C28): admin and CLI recovery actions are
// notified with their own texts, never the generic one.
import { describe, it, expect } from "vitest";
import { decodeSecurityNotice } from "@/lib/types";
import { noticeText, pushText } from "../noticeText";
import { event, notice } from "./fixtures";

const generic = noticeText(notice({}, [event({ event_type: "unknown" as never })]), "me");
const genericPush = pushText(notice({}, [event({ event_type: "unknown" as never })]), "me");

function decoded(type: string, details: Record<string, unknown>) {
  const raw = notice({ event_type: type as never }, [
    event({ event_type: type as never, actor: { kind: "admin", user_id: "a1", username: "root" } as never, details }),
  ]);
  const n = decodeSecurityNotice(raw);
  if (!n) throw new Error("not decoded");
  return n;
}

describe("C28 amendment event types", () => {
  const cases: [string, Record<string, unknown>, RegExp][] = [
    ["sessions_ended", { count: 3, by: "admin" }, /sessions/i],
    ["factor_restored", { factor: "totp", by: "cli" }, /allowed again/i],
    ["throttle_reset", { factor: "password", by: "admin" }, /pause/i],
    ["may_approve_changed", { may_approve: false, by: "admin" }, /confirm sign-ins/i],
  ];
  for (const [type, details, words] of cases) {
    it(`${type} has its own notice and push text`, () => {
      const n = decoded(type, details);
      expect(n.event_type).toBe(type);
      const text = noticeText(n, "me");
      expect(text).not.toBe(generic);
      expect(text).toMatch(words);
      expect(pushText(n, "me")).not.toBe(genericPush);
    });
  }
});
