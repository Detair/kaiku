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
      .mockRejectedValue(new Error("NotFoundError"));
    document.body.appendChild(el);
    (adapter as unknown as { remoteStreams: Map<string, unknown> }).remoteStreams.set("u", {
      id: "s1",
    });
    const r = await adapter.setOutputDevice("stale");
    el.remove();
    expect(r.ok).toBe(false);
  });
});
