/**
 * A8: a short ICE outage is "reconnecting", never the end of the call.
 */
import { describe, expect, it } from "vitest";
import { mapPublisherState } from "../browser";
import { mapNativeVoiceState } from "../tauri";

describe("browser publisher state mapping", () => {
  it("keeps the call on a short ICE outage", () => {
    expect(mapPublisherState("disconnected")).toEqual({ state: "reconnecting", lost: false });
  });

  it("ends only on closed", () => {
    expect(mapPublisherState("closed")).toEqual({ state: "disconnected", lost: false });
  });

  it("reports failed as lost so the store rejoins", () => {
    expect(mapPublisherState("failed")).toEqual({ state: "reconnecting", lost: true });
  });

  it("maps connected and the setup states", () => {
    expect(mapPublisherState("connected").state).toBe("connected");
    expect(mapPublisherState("new").state).toBe("connecting");
    expect(mapPublisherState("connecting").state).toBe("connecting");
  });
});

describe("native voice state mapping", () => {
  it("maps the Rust connection states", () => {
    expect(mapNativeVoiceState("Connected")).toEqual({ state: "connected", lost: false });
    expect(mapNativeVoiceState("Connecting")).toEqual({ state: "connecting", lost: false });
    expect(mapNativeVoiceState("Reconnecting")).toEqual({ state: "reconnecting", lost: false });
    expect(mapNativeVoiceState("Failed")).toEqual({ state: "reconnecting", lost: true });
    expect(mapNativeVoiceState("Disconnected")).toEqual({ state: "disconnected", lost: false });
  });
});
