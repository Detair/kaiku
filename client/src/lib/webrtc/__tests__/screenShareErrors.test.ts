import { describe, expect, it } from "vitest";
import { screenShareCheckErrorText } from "../screenShareErrors";
import { t } from "@/i18n";

describe("screenShareCheckErrorText", () => {
  it.each([
    ["no_permission", "voice.screenShare.error.noPermission"],
    ["NO_PERMISSION", "voice.screenShare.error.noPermission"],
    ["limit_reached", "voice.screenShare.error.limitReached"],
    ["LIMIT_REACHED", "voice.screenShare.error.limitReached"],
    ["not_in_channel", "voice.screenShare.error.notInChannel"],
    ["NOT_IN_CHANNEL", "voice.screenShare.error.notInChannel"],
  ] as const)("%s", (code, key) => {
    expect(screenShareCheckErrorText(code)).toBe(t(key));
  });
  it("unknown / missing codes give a generic text, never the code", () => {
    expect(screenShareCheckErrorText("WHAT")).toBe(t("voice.screenShare.error.generic"));
    expect(screenShareCheckErrorText(undefined)).toBe(t("voice.screenShare.error.generic"));
  });
});
