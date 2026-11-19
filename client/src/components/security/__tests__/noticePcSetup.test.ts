// "Set up this PC" notices: its two event types have their own texts, and the
// new-passkey notice names the computer and the approving phone.
import { describe, it, expect } from "vitest";
import { decodeSecurityNotice } from "@/lib/types";
import { eventText, noticeText, pushText } from "../noticeText";
import { event, notice } from "./fixtures";

const generic = noticeText(notice({}, [event({ event_type: "unknown" as never })]), "me");
const genericPush = pushText(notice({}, [event({ event_type: "unknown" as never })]), "me");

function decoded(type: string, details: Record<string, unknown>) {
  const n = decodeSecurityNotice(notice({ event_type: type as never }, [event({ event_type: type as never, details })]));
  if (!n) throw new Error("not decoded");
  return n;
}

describe("Set up this PC notices", () => {
  const cases: [string, RegExp][] = [
    ["pc_setup_reported", /didn't start this/i],
    ["pc_setup_failed", /wrong codes/i],
  ];
  for (const [type, words] of cases) {
    it(`${type} has its own notice and push text`, () => {
      const n = decoded(type, { attempt_id: "a1" });
      expect(n.event_type).toBe(type);
      const text = noticeText(n, "me");
      expect(text).not.toBe(generic);
      expect(text).toMatch(words);
      expect(pushText(n, "me")).not.toBe(genericPush);
    });
  }

  it("the new passkey of a PC setup names the computer and the phone", () => {
    const details = {
      type: "passkey",
      via: "pc_setup",
      client_description: "Firefox on Linux",
      approving_device_label: "Pixel 8",
    };
    const n = decoded("authenticator_added", details);
    const text = noticeText(n, "me");
    expect(text).toContain("Firefox on Linux");
    expect(text).toContain("Pixel 8");
    expect(eventText(n.events[0], n.subject, "me")).toContain("Pixel 8");
  });

  it("any other new passkey keeps its usual text", () => {
    const n = decoded("authenticator_added", { type: "passkey" });
    expect(noticeText(n, "me")).not.toContain("confirmed with");
  });
});
