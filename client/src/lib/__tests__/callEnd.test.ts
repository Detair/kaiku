import { describe, expect, it } from "vitest";
import { t } from "@/i18n";
import { HttpError } from "@/lib/tauri/common";
import { endReasonText, isCallAlreadyGoneError, isCallGlareError } from "../callEnd";

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

describe("call error classification", () => {
  it("recognises call_not_found in any case, by message or code", () => {
    expect(isCallAlreadyGoneError(new HttpError(404, "x", "CALL_NOT_FOUND"))).toBe(true);
    expect(isCallAlreadyGoneError(new HttpError(404, "x", "call_not_found"))).toBe(true);
    expect(isCallAlreadyGoneError(new Error("Call Not Found"))).toBe(true);
    expect(isCallAlreadyGoneError(new HttpError(500, "boom"))).toBe(false);
  });

  it("recognises glare (409 call_already_exists) but not contention", () => {
    expect(isCallGlareError(new HttpError(409, "x", "call_already_exists"))).toBe(true);
    expect(isCallGlareError(new HttpError(409, "x", "CALL_ALREADY_EXISTS"))).toBe(true);
    expect(isCallGlareError(new HttpError(409, "x", "CALL_CONFLICT"))).toBe(false);
  });
});
