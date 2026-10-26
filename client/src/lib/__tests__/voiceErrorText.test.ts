import { describe, it, expect } from "vitest";
import { voiceErrorText } from "../webrtc/voiceErrorText";

describe("voiceErrorText", () => {
  it("maps known device errors to catalog text, not the adapter message", () => {
    expect(voiceErrorText({ type: "permission_denied", message: "NotAllowedError" })).toBe(
      "Microphone access denied. Please allow microphone in browser settings.",
    );
    expect(voiceErrorText({ type: "device_not_found", message: "raw" })).toBe(
      "No microphone found. Please connect a microphone.",
    );
    expect(voiceErrorText({ type: "device_in_use", message: "raw" })).toBe(
      "Microphone is being used by another app.",
    );
  });

  it("wraps opaque adapter messages in a translated sentence", () => {
    expect(voiceErrorText({ type: "unknown", message: "boom" })).toBe("Audio device error: boom");
    expect(voiceErrorText({ type: "ice_failed", message: "boom" })).toBe("Voice error: boom");
  });
});
