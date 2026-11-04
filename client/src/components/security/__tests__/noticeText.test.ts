import { describe, it, expect } from "vitest";
import { NOTICE_KEYS, PUSH_KEYS, noticeText, pushText } from "../noticeText";
import { SECURITY_EVENT_TYPES } from "@/lib/types";
import { en } from "@/i18n/en";
import { event, notice } from "./fixtures";

describe("notice text", () => {
  it("every event type has a literal key and unknown uses generic", () => {
    for (const type of SECURITY_EVENT_TYPES) {
      expect(NOTICE_KEYS[type], type).toMatch(/^security\.notice\./);
      expect(PUSH_KEYS[type], type).toMatch(/^security\.push\./);
      expect(NOTICE_KEYS[type] in en).toBe(true);
    }
    expect(NOTICE_KEYS.unknown).toBe("security.notice.generic");
    expect(PUSH_KEYS.unknown).toBe("security.push.generic");
    expect(NOTICE_KEYS.sign_in).toBe("security.notice.signIn");
  });

  it("notice text fills subject actor device count", () => {
    const n = notice(
      { subject: { user_id: "oma", username: "oma", display_name: "Oma" }, count: 1, event_type: "authenticator_removed", severity: "high" },
      [event({ event_type: "authenticator_removed", actor: { kind: "admin", user_id: "a1", username: "fabian" }, authenticator: { id: "d1", type: "key_device", label: "Pixel 8" } })],
    );
    expect(noticeText(n, "me")).toBe("fabian removed Pixel 8 from Oma's account.");
    const mine = notice({ count: 3 }, [event(), event({ id: "e2" }), event({ id: "e3" })]);
    expect(noticeText(mine, "me")).toBe("New sign-in to your account (3 times).");
    expect(noticeText(notice({}, [event()]), "me")).toBe("New sign-in to your account.");
    expect(pushText(notice({ event_type: "unknown" }, [event({ event_type: "unknown" })]), "me")).toBe(
      "Kaiku has a security notice for you.",
    );
  });
});
