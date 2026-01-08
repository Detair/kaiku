/**
 * Browser Voice Adapter
 *
 * WebRTC implementation using browser APIs (RTCPeerConnection, getUserMedia)
 */

import type {
  VoiceAdapter,
  VoiceConnectionState,
  VoiceError,
  VoiceResult,
  VoiceAdapterEvents,
  AudioDeviceList,
  RemoteTrack,
} from "./types";

export class BrowserVoiceAdapter implements VoiceAdapter {
  private state: VoiceConnectionState = "disconnected";
  private channelId: string | null = null;
  private muted = false;
  private deafened = false;

  // WebRTC
  private peerConnection: RTCPeerConnection | null = null;
  private localStream: MediaStream | null = null;
  private remoteStreams = new Map<string, MediaStream>();

  // Mic test
  private micTestStream: MediaStream | null = null;
  private micTestAnalyser: AnalyserNode | null = null;
  private audioContext: AudioContext | null = null;

  // Event handlers
  private eventHandlers: Partial<VoiceAdapterEvents> = {};

  // Selected devices
  private inputDeviceId: string | null = null;
  // private _outputDeviceId: string | null = null; // TODO: Implement output device selection

  constructor() {
    console.log("[BrowserVoiceAdapter] Initialized");
  }

  // Lifecycle methods

  async join(channelId: string): Promise<VoiceResult<void>> {
    console.log(`[BrowserVoiceAdapter] Joining channel: ${channelId}`);

    if (this.peerConnection) {
      return {
        ok: false,
        error: {
          type: "already_connected",
          channelId: this.channelId || "unknown",
        },
      };
    }

    try {
      this.channelId = channelId;
      this.setState("requesting_media");

      // Get microphone access
      const constraints: MediaStreamConstraints = {
        audio: this.inputDeviceId
          ? { deviceId: { exact: this.inputDeviceId } }
          : true,
      };

      this.localStream = await navigator.mediaDevices.getUserMedia(constraints);
      console.log("[BrowserVoiceAdapter] Got local stream");

      this.setState("connecting");

      // Create peer connection
      const config: RTCConfiguration = {
        iceServers: [{ urls: "stun:stun.l.google.com:19302" }],
      };

      this.peerConnection = new RTCPeerConnection(config);
      this.setupPeerConnectionHandlers();

      // Add local tracks
      this.localStream.getTracks().forEach((track) => {
        this.peerConnection!.addTrack(track, this.localStream!);
      });

      console.log("[BrowserVoiceAdapter] Peer connection created, sending voice_join");

      // Ensure WebSocket is connected before sending
      const { wsSend, wsStatus, wsConnect } = await import("@/lib/tauri");

      // Check WebSocket status
      let status = await wsStatus();
      console.log("[BrowserVoiceAdapter] WebSocket status:", status);

      // If disconnected, try to connect
      if (status.type === "disconnected") {
        console.log("[BrowserVoiceAdapter] WebSocket disconnected, attempting to connect...");
        try {
          await wsConnect();
          status = await wsStatus();
          console.log("[BrowserVoiceAdapter] WebSocket reconnected:", status);
        } catch (err) {
          console.error("[BrowserVoiceAdapter] Failed to connect WebSocket:", err);
          throw new Error("Failed to connect to server. Please refresh the page.");
        }
      }

      // Wait for WebSocket to be connected (with timeout)
      let attempts = 0;
      const maxAttempts = 30; // 3 seconds max
      while (attempts < maxAttempts && status.type !== "connected") {
        if (status.type === "disconnected") {
          throw new Error("WebSocket disconnected unexpectedly.");
        }
        await new Promise(resolve => setTimeout(resolve, 100));
        status = await wsStatus();
        attempts++;
      }

      if (status.type !== "connected") {
        throw new Error("WebSocket connection timeout. Status: " + status.type);
      }

      console.log("[BrowserVoiceAdapter] WebSocket ready, sending voice_join");

      // Send voice_join message to server
      await wsSend({
        type: "voice_join",
        channel_id: channelId,
      });

      console.log("[BrowserVoiceAdapter] Waiting for offer from server");

      return { ok: true, value: undefined };
    } catch (err) {
      console.error("[BrowserVoiceAdapter] Error during join:", err);
      this.cleanup();
      this.setState("disconnected");
      this.channelId = null;
      return { ok: false, error: this.mapMediaError(err) };
    }
  }

  async leave(): Promise<VoiceResult<void>> {
    console.log("[BrowserVoiceAdapter] Leaving voice");

    // Send voice_leave message to server
    if (this.channelId) {
      const { wsSend } = await import("@/lib/tauri");
      await wsSend({
        type: "voice_leave",
        channel_id: this.channelId,
      });
    }

    this.cleanup();
    this.setState("disconnected");
    this.channelId = null;

    return { ok: true, value: undefined };
  }

  // Audio control

  async setMute(muted: boolean): Promise<VoiceResult<void>> {
    console.log(`[BrowserVoiceAdapter] Set mute: ${muted}`);

    this.muted = muted;

    if (this.localStream) {
      this.localStream.getAudioTracks().forEach((track) => {
        track.enabled = !muted;
      });
    }

    // Notify server
    if (this.channelId) {
      const { wsSend } = await import("@/lib/tauri");
      await wsSend({
        type: muted ? "voice_mute" : "voice_unmute",
        channel_id: this.channelId,
      });
    }

    this.eventHandlers.onLocalMuteChange?.(muted);

    return { ok: true, value: undefined };
  }

  async setDeafen(deafened: boolean): Promise<VoiceResult<void>> {
    console.log(`[BrowserVoiceAdapter] Set deafen: ${deafened}`);

    this.deafened = deafened;

    // Also mute when deafened
    if (deafened) {
      await this.setMute(true);
    }

    // Mute all remote streams
    this.remoteStreams.forEach((stream) => {
      stream.getAudioTracks().forEach((track) => {
        track.enabled = !deafened;
      });
    });

    return { ok: true, value: undefined };
  }

  // Signaling

  async handleOffer(
    channelId: string,
    sdp: string
  ): Promise<VoiceResult<string>> {
    console.log(`[BrowserVoiceAdapter] Handling offer for channel: ${channelId}`);

    if (!this.peerConnection) {
      return {
        ok: false,
        error: { type: "not_connected" },
      };
    }

    if (this.channelId !== channelId) {
      return {
        ok: false,
        error: {
          type: "server_rejected",
          code: "WRONG_CHANNEL",
          message: `Received offer for wrong channel: ${channelId}`,
        },
      };
    }

    try {
      // Set remote description
      await this.peerConnection.setRemoteDescription({
        type: "offer",
        sdp,
      });

      // Create answer
      const answer = await this.peerConnection.createAnswer();
      await this.peerConnection.setLocalDescription(answer);

      console.log("[BrowserVoiceAdapter] Answer created");

      return { ok: true, value: answer.sdp! };
    } catch (err) {
      return {
        ok: false,
        error: {
          type: "connection_failed",
          reason: err instanceof Error ? err.message : String(err),
          retriable: true,
        },
      };
    }
  }

  async handleIceCandidate(
    channelId: string,
    candidate: string
  ): Promise<VoiceResult<void>> {
    console.log(`[BrowserVoiceAdapter] Handling ICE candidate`);

    if (!this.peerConnection) {
      return {
        ok: false,
        error: { type: "not_connected" },
      };
    }

    if (this.channelId !== channelId) {
      return {
        ok: false,
        error: {
          type: "server_rejected",
          code: "WRONG_CHANNEL",
          message: `Received ICE candidate for wrong channel: ${channelId}`,
        },
      };
    }

    try {
      const candidateInit = JSON.parse(candidate);
      await this.peerConnection.addIceCandidate(
        new RTCIceCandidate(candidateInit)
      );
      console.log("[BrowserVoiceAdapter] ICE candidate added");
      return { ok: true, value: undefined };
    } catch (err) {
      return {
        ok: false,
        error: {
          type: "ice_failed",
          message: err instanceof Error ? err.message : String(err),
        },
      };
    }
  }

  // State methods

  getState(): VoiceConnectionState {
    return this.state;
  }

  getChannelId(): string | null {
    return this.channelId;
  }

  isMuted(): boolean {
    return this.muted;
  }

  isDeafened(): boolean {
    return this.deafened;
  }

  setEventHandlers(handlers: Partial<VoiceAdapterEvents>): void {
    this.eventHandlers = { ...this.eventHandlers, ...handlers };
  }

  // Microphone test

  async startMicTest(deviceId?: string): Promise<VoiceResult<void>> {
    console.log("[BrowserVoiceAdapter] Starting mic test");

    try {
      // Stop any existing test
      await this.stopMicTest();

      // Get microphone stream
      const constraints: MediaStreamConstraints = {
        audio: deviceId ? { deviceId: { exact: deviceId } } : true,
      };
      this.micTestStream = await navigator.mediaDevices.getUserMedia(
        constraints
      );

      // Set up audio analysis for level metering
      this.audioContext = new AudioContext();
      const source = this.audioContext.createMediaStreamSource(
        this.micTestStream
      );
      this.micTestAnalyser = this.audioContext.createAnalyser();
      this.micTestAnalyser.fftSize = 256;
      source.connect(this.micTestAnalyser);

      return { ok: true, value: undefined };
    } catch (err) {
      return { ok: false, error: this.mapMediaError(err) };
    }
  }

  async stopMicTest(): Promise<VoiceResult<void>> {
    console.log("[BrowserVoiceAdapter] Stopping mic test");

    if (this.micTestStream) {
      this.micTestStream.getTracks().forEach((track) => track.stop());
      this.micTestStream = null;
    }
    if (this.audioContext) {
      await this.audioContext.close();
      this.audioContext = null;
    }
    this.micTestAnalyser = null;
    return { ok: true, value: undefined };
  }

  getMicTestLevel(): number {
    if (!this.micTestAnalyser) return 0;

    const dataArray = new Uint8Array(this.micTestAnalyser.frequencyBinCount);
    this.micTestAnalyser.getByteFrequencyData(dataArray);

    // Calculate RMS volume
    const sum = dataArray.reduce((acc, val) => acc + val, 0);
    const average = sum / dataArray.length;

    // Normalize to 0-100
    return Math.min(100, Math.round((average / 255) * 100 * 2));
  }

  // Device enumeration

  async getAudioDevices(): Promise<VoiceResult<AudioDeviceList>> {
    try {
      // Need to request permission first to get device labels
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      stream.getTracks().forEach((t) => t.stop());

      const devices = await navigator.mediaDevices.enumerateDevices();

      const inputs = devices
        .filter((d) => d.kind === "audioinput")
        .map((d) => ({
          deviceId: d.deviceId,
          label: d.label || `Microphone ${d.deviceId.slice(0, 8)}`,
          isDefault: d.deviceId === "default",
        }));

      const outputs = devices
        .filter((d) => d.kind === "audiooutput")
        .map((d) => ({
          deviceId: d.deviceId,
          label: d.label || `Speaker ${d.deviceId.slice(0, 8)}`,
          isDefault: d.deviceId === "default",
        }));

      return { ok: true, value: { inputs, outputs } };
    } catch (err) {
      return { ok: false, error: this.mapMediaError(err) };
    }
  }

  async setInputDevice(deviceId: string): Promise<VoiceResult<void>> {
    console.log(`[BrowserVoiceAdapter] Setting input device: ${deviceId}`);
    this.inputDeviceId = deviceId;

    // If already in a call, restart the stream with the new device
    if (this.localStream && this.peerConnection) {
      try {
        // Stop old tracks
        this.localStream.getTracks().forEach((track) => track.stop());

        // Get new stream
        const constraints: MediaStreamConstraints = {
          audio: { deviceId: { exact: deviceId } },
        };
        this.localStream = await navigator.mediaDevices.getUserMedia(
          constraints
        );

        // Replace tracks in peer connection
        const sender = this.peerConnection
          .getSenders()
          .find((s) => s.track?.kind === "audio");
        if (sender) {
          const newTrack = this.localStream.getAudioTracks()[0];
          await sender.replaceTrack(newTrack);
        }

        return { ok: true, value: undefined };
      } catch (err) {
        return { ok: false, error: this.mapMediaError(err) };
      }
    }

    return { ok: true, value: undefined };
  }

  async setOutputDevice(deviceId: string): Promise<VoiceResult<void>> {
    console.log(`[BrowserVoiceAdapter] Setting output device: ${deviceId}`);
    // this._outputDeviceId = deviceId; // TODO: Store selected output device

    // Set output device for all remote streams
    // Note: This requires the experimental setSinkId API
    try {
      for (const stream of this.remoteStreams.values()) {
        const audioElements = document.querySelectorAll(
          `audio[data-stream-id="${stream.id}"]`
        );
        audioElements.forEach((audio) => {
          if ("setSinkId" in audio) {
            (audio as any).setSinkId(deviceId);
          }
        });
      }
      return { ok: true, value: undefined };
    } catch (err) {
      return {
        ok: false,
        error: {
          type: "device_not_found",
          message: err instanceof Error ? err.message : String(err),
        },
      };
    }
  }

  // Cleanup

  dispose(): void {
    console.log("[BrowserVoiceAdapter] Disposing");
    this.cleanup();
  }

  // Private helper methods

  private setState(state: VoiceConnectionState) {
    this.state = state;
    this.eventHandlers.onStateChange?.(state);
  }

  private setupPeerConnectionHandlers() {
    if (!this.peerConnection) return;

    // ICE candidate handler
    this.peerConnection.onicecandidate = (event) => {
      if (event.candidate) {
        const candidateJson = JSON.stringify(event.candidate.toJSON());
        this.eventHandlers.onIceCandidate?.(candidateJson);
      }
    };

    // Connection state change
    this.peerConnection.onconnectionstatechange = () => {
      const state = this.peerConnection!.connectionState;
      console.log(`[BrowserVoiceAdapter] Connection state: ${state}`);

      switch (state) {
        case "connected":
          this.setState("connected");
          break;
        case "disconnected":
        case "closed":
          this.setState("disconnected");
          break;
        case "failed":
          this.setState("disconnected");
          this.eventHandlers.onError?.({
            type: "connection_failed",
            reason: "Peer connection failed",
            retriable: true,
          });
          break;
        case "connecting":
        case "new":
          this.setState("connecting");
          break;
      }
    };

    // Remote track handler
    this.peerConnection.ontrack = (event) => {
      console.log("[BrowserVoiceAdapter] Remote track received");

      const stream = event.streams[0];
      const userId = stream.id; // TODO: Get actual user ID from track metadata

      this.remoteStreams.set(userId, stream);

      const remoteTrack: RemoteTrack = {
        trackId: event.track.id,
        userId,
        stream,
        muted: false,
      };

      this.eventHandlers.onRemoteTrack?.(remoteTrack);

      // Handle track ending
      event.track.onended = () => {
        console.log("[BrowserVoiceAdapter] Remote track ended");
        this.remoteStreams.delete(userId);
        this.eventHandlers.onRemoteTrackRemoved?.(userId);
      };
    };
  }

  private cleanup() {
    // Stop local stream
    if (this.localStream) {
      this.localStream.getTracks().forEach((track) => track.stop());
      this.localStream = null;
    }

    // Close peer connection
    if (this.peerConnection) {
      this.peerConnection.close();
      this.peerConnection = null;
    }

    // Clear remote streams
    this.remoteStreams.clear();

    // Stop mic test if running
    this.stopMicTest();
  }

  private mapMediaError(err: unknown): VoiceError {
    if (err instanceof DOMException) {
      switch (err.name) {
        case "NotAllowedError":
        case "PermissionDeniedError":
          return {
            type: "permission_denied",
            message:
              "Microphone access denied. Please allow microphone in browser settings.",
          };
        case "NotFoundError":
        case "DevicesNotFoundError":
          return {
            type: "device_not_found",
            message: "No microphone found. Please connect a microphone.",
          };
        case "NotReadableError":
        case "TrackStartError":
          return {
            type: "device_in_use",
            message: "Microphone is being used by another app.",
          };
        default:
          return {
            type: "unknown",
            message: err.message || "An unknown error occurred",
          };
      }
    }

    return {
      type: "unknown",
      message: err instanceof Error ? err.message : String(err),
    };
  }
}
