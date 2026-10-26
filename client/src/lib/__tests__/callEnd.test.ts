import { describe, expect, it } from "vitest";
import { t } from "@/i18n";
import { endReasonText } from "../callEnd";

describe("endReasonText", () => {
  it.each([
    ["cancelled", "voice.call.ended.cancelled"],
    ["declined", "voice.call.ended.declined"],
    ["all_declined", "voice.call.ended.allDeclined"],
    ["no_answer", "voice.call.ended.noAnswer"],
    ["last_left", "voice.call.ended.lastLeft"],
    ["something_new", "voice.call.ended.default"],
  ] as const)("maps %s", (reason, key) => {
    expect(endReasonText(reason)).toBe(t(key));
  });

  it("uses distinct plain texts", () => {
    expect(endReasonText("no_answer")).toBe("No answer");
    expect(endReasonText("all_declined")).not.toBe(endReasonText("last_left"));
  });
});
