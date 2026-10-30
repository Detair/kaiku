import { describe, it, expect, afterEach, vi } from "vitest";
import { BrowserVoiceAdapter } from "../browser";

function dev(kind: MediaDeviceKind, deviceId: string, label: string): MediaDeviceInfo {
  return { kind, deviceId, label, groupId: "", toJSON: () => ({}) } as MediaDeviceInfo;
}

function stubMediaDevices(devices: MediaDeviceInfo[]) {
  const getUserMedia = vi.fn().mockResolvedValue({ getTracks: () => [{ stop: vi.fn() }] });
  const enumerateDevices = vi.fn().mockResolvedValue(devices);
  vi.stubGlobal("navigator", { mediaDevices: { getUserMedia, enumerateDevices } });
  return { getUserMedia, enumerateDevices };
}

describe("BrowserVoiceAdapter device enumeration", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("does not request the microphone when listing devices", async () => {
    const { getUserMedia, enumerateDevices } = stubMediaDevices([
      dev("audioinput", "a", "USB Mic"),
      dev("audiooutput", "b", "Speakers"),
    ]);
    const r = await new BrowserVoiceAdapter().getAudioDevices();
    expect(getUserMedia).not.toHaveBeenCalled();
    expect(enumerateDevices).toHaveBeenCalled();
    expect(r.ok && r.value.labelsAvailable).toBe(true);
  });

  it("flags missing labels and drops id-less placeholders", async () => {
    const { getUserMedia } = stubMediaDevices([
      dev("audioinput", "", ""),
      dev("audiooutput", "", ""),
    ]);
    const r = await new BrowserVoiceAdapter().getAudioDevices();
    expect(getUserMedia).not.toHaveBeenCalled();
    expect(r.ok).toBe(true);
    if (r.ok) {
      expect(r.value.labelsAvailable).toBe(false);
      expect(r.value.inputs).toEqual([]);
    }
  });

  it("enumerates before stopping the permission tracks", async () => {
    const order: string[] = [];
    const stop = vi.fn(() => order.push("stop"));
    vi.stubGlobal("navigator", {
      mediaDevices: {
        getUserMedia: vi.fn().mockResolvedValue({ getTracks: () => [{ stop }] }),
        enumerateDevices: vi.fn(async () => {
          order.push("enumerate");
          return [dev("audioinput", "a", "USB Mic")];
        }),
      },
    });
    await new BrowserVoiceAdapter().requestDeviceLabels();
    expect(order).toEqual(["enumerate", "stop"]);
  });

  it("requests the microphone only via requestDeviceLabels", async () => {
    const { getUserMedia } = stubMediaDevices([dev("audioinput", "a", "USB Mic")]);
    const r = await new BrowserVoiceAdapter().requestDeviceLabels();
    expect(getUserMedia).toHaveBeenCalledTimes(1);
    expect(r.ok && r.value.inputs[0].label).toBe("USB Mic");
  });
});

describe("BrowserVoiceAdapter stale device handling", () => {
  afterEach(() => vi.unstubAllGlobals());

  function inCall() {
    const adapter = new BrowserVoiceAdapter();
    const oldStop = vi.fn();
    const oldStream = { getTracks: () => [{ stop: oldStop }], getAudioTracks: () => [{}] };
    const replaceTrack = vi.fn().mockResolvedValue(undefined);
    const priv = adapter as unknown as Record<string, unknown>;
    priv.localStream = oldStream;
    priv.inputDeviceId = "good";
    priv.publisherState = {
      pc: { getSenders: () => [{ track: { kind: "audio" }, replaceTrack }] },
    };
    return { adapter, priv, oldStop, oldStream, replaceTrack };
  }

  it("keeps the working microphone and id when the new device is gone", async () => {
    const { adapter, priv, oldStop, oldStream, replaceTrack } = inCall();
    const getUserMedia = vi
      .fn()
      .mockRejectedValue(Object.assign(new Error("gone"), { name: "OverconstrainedError" }));
    vi.stubGlobal("navigator", { mediaDevices: { getUserMedia } });

    const r = await adapter.setInputDevice("stale");
    expect(r.ok).toBe(false);
    expect(oldStop).not.toHaveBeenCalled();
    expect(priv.localStream).toBe(oldStream);
    expect(priv.inputDeviceId).toBe("good");
    expect(replaceTrack).not.toHaveBeenCalled();
  });

  it("switches and remembers the id once the new device opens", async () => {
    const { adapter, priv, oldStop, replaceTrack } = inCall();
    const newStream = { getTracks: () => [], getAudioTracks: () => [{ id: "new" }] };
    vi.stubGlobal("navigator", {
      mediaDevices: { getUserMedia: vi.fn().mockResolvedValue(newStream) },
    });
    const r = await adapter.setInputDevice("fresh");
    expect(r.ok).toBe(true);
    expect(oldStop).toHaveBeenCalled();
    expect(priv.inputDeviceId).toBe("fresh");
    expect(replaceTrack).toHaveBeenCalled();
  });

  it("reports a stale speaker so it can be forgotten", async () => {
    const adapter = new BrowserVoiceAdapter();
    const el = document.createElement("audio");
    el.dataset.streamId = "s1";
    (el as unknown as { setSinkId: unknown }).setSinkId = vi
      .fn()
      .mockRejectedValue(Object.assign(new Error("gone"), { name: "NotFoundError" }));
    document.body.appendChild(el);
    (adapter as unknown as { remoteStreams: Map<string, unknown> }).remoteStreams.set("u", {
      id: "s1",
    });
    const r = await adapter.setOutputDevice("stale");
    el.remove();
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.error.type).toBe("device_not_found");
  });

  it("does not call a transient setSinkId failure device_not_found", async () => {
    const adapter = new BrowserVoiceAdapter();
    const el = document.createElement("audio");
    el.dataset.streamId = "s2";
    (el as unknown as { setSinkId: unknown }).setSinkId = vi
      .fn()
      .mockRejectedValue(Object.assign(new Error("denied"), { name: "NotAllowedError" }));
    document.body.appendChild(el);
    (adapter as unknown as { remoteStreams: Map<string, unknown> }).remoteStreams.set("u", {
      id: "s2",
    });
    const r = await adapter.setOutputDevice("x");
    el.remove();
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.error.type).not.toBe("device_not_found");
  });

  it("maps an in-call OverconstrainedError to device_not_found", async () => {
    const { adapter } = inCall();
    vi.stubGlobal("navigator", {
      mediaDevices: {
        getUserMedia: vi
          .fn()
          .mockRejectedValue(Object.assign(new Error("x"), { name: "OverconstrainedError" })),
      },
    });
    const r = await adapter.setInputDevice("stale");
    expect(!r.ok && r.error.type).toBe("device_not_found");
  });
});

describe("BrowserVoiceAdapter stored device preferences", () => {
  afterEach(() => vi.unstubAllGlobals());

  function sinkEl(setSinkId: ReturnType<typeof vi.fn>): HTMLAudioElement {
    const el = document.createElement("audio");
    (el as unknown as { setSinkId: unknown }).setSinkId = setSinkId;
    return el;
  }

  it("records the input device before join without opening the microphone", async () => {
    const getUserMedia = vi.fn();
    vi.stubGlobal("navigator", { mediaDevices: { getUserMedia } });
    const adapter = new BrowserVoiceAdapter();
    const r = await adapter.setInputDevice("usb");
    expect(r.ok).toBe(true);
    expect(getUserMedia).not.toHaveBeenCalled();
    expect(adapter.getInputDevice()).toBe("usb");
  });

  it("join opens the preselected microphone directly", async () => {
    const getUserMedia = vi.fn().mockRejectedValue(new Error("stop here"));
    vi.stubGlobal("navigator", { mediaDevices: { getUserMedia } });
    const adapter = new BrowserVoiceAdapter();
    await adapter.setInputDevice("usb");
    await adapter.join("ch");
    const audio = getUserMedia.mock.calls[0][0].audio as MediaTrackConstraints;
    expect(audio.deviceId).toEqual({ exact: "usb" });
  });

  it("join falls back to the default mic and forgets a gone device", async () => {
    const gone = Object.assign(new Error("gone"), { name: "OverconstrainedError" });
    const getUserMedia = vi
      .fn()
      .mockRejectedValueOnce(gone)
      .mockRejectedValueOnce(new Error("stop here"));
    vi.stubGlobal("navigator", { mediaDevices: { getUserMedia } });
    const adapter = new BrowserVoiceAdapter();
    await adapter.setInputDevice("stale");
    await adapter.join("ch");
    expect(getUserMedia).toHaveBeenCalledTimes(2);
    expect((getUserMedia.mock.calls[1][0].audio as MediaTrackConstraints).deviceId).toBeUndefined();
    expect(adapter.getInputDevice()).toBeNull();
  });

  it("join keeps the preselected device on a transient failure", async () => {
    const busy = Object.assign(new Error("busy"), { name: "NotReadableError" });
    vi.stubGlobal("navigator", {
      mediaDevices: { getUserMedia: vi.fn().mockRejectedValue(busy) },
    });
    const adapter = new BrowserVoiceAdapter();
    await adapter.setInputDevice("usb");
    const r = await adapter.join("ch");
    expect(r.ok).toBe(false);
    expect(adapter.getInputDevice()).toBe("usb");
  });

  it("remembers the speaker and applies it to later remote audio", async () => {
    const adapter = new BrowserVoiceAdapter();
    expect((await adapter.setOutputDevice("headset")).ok).toBe(true);
    const setSinkId = vi.fn().mockResolvedValue(undefined);
    const r = await adapter.applyOutputDeviceTo(sinkEl(setSinkId));
    expect(r.ok).toBe(true);
    expect(setSinkId).toHaveBeenCalledWith("headset");
  });

  it("leaves later remote audio on the default output when no speaker is set", async () => {
    const setSinkId = vi.fn();
    const r = await new BrowserVoiceAdapter().applyOutputDeviceTo(sinkEl(setSinkId));
    expect(r.ok).toBe(true);
    expect(setSinkId).not.toHaveBeenCalled();
  });

  it("reports a stale remembered speaker once and then uses the default", async () => {
    const adapter = new BrowserVoiceAdapter();
    await adapter.setOutputDevice("stale");
    const notFound = vi
      .fn()
      .mockRejectedValue(Object.assign(new Error("gone"), { name: "NotFoundError" }));
    const r = await adapter.applyOutputDeviceTo(sinkEl(notFound));
    expect(!r.ok && r.error.type).toBe("device_not_found");

    const next = vi.fn().mockResolvedValue(undefined);
    expect((await adapter.applyOutputDeviceTo(sinkEl(next))).ok).toBe(true);
    expect(next).not.toHaveBeenCalled();
  });

  it("keeps the remembered speaker on a transient setSinkId failure", async () => {
    const adapter = new BrowserVoiceAdapter();
    await adapter.setOutputDevice("headset");
    const denied = vi
      .fn()
      .mockRejectedValue(Object.assign(new Error("denied"), { name: "NotAllowedError" }));
    const r = await adapter.applyOutputDeviceTo(sinkEl(denied));
    expect(!r.ok && r.error.type).not.toBe("device_not_found");
    const next = vi.fn().mockResolvedValue(undefined);
    await adapter.applyOutputDeviceTo(sinkEl(next));
    expect(next).toHaveBeenCalledWith("headset");
  });

  it("is a no-op where setSinkId is unsupported", async () => {
    const adapter = new BrowserVoiceAdapter();
    await adapter.setOutputDevice("headset");
    const r = await adapter.applyOutputDeviceTo(document.createElement("audio"));
    expect(r.ok).toBe(true);
  });
});
