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
  ScreenShareOptions,
  ScreenShareQuality,
  WebcamOptions,
  ConnectionMetrics,
  QualityLevel,
} from "./types";
import * as Sentry from "@sentry/browser";

/** HTMLAudioElement extended with the non-standard setSinkId API (Chrome/Edge). */
type AudioElementWithSinkId = HTMLAudioElement & {
  setSinkId(id: string): Promise<void>;
};

export class BrowserVoiceAdapter implements VoiceAdapter {
  private state: VoiceConnectionState = "disconnected";
  private channelId: string | null = null;
  private muted = false;
  private deafened = false;
  private noiseSuppression = true;

  // WebRTC
  private peerConnection: RTCPeerConnection | null = null;
  private localStream: MediaStream | null = null;
  private remoteStreams = new Map<string, MediaStream>();

  // Screen share state
  private screenShareStream: MediaStream | null = null;
  private screenShareTrack: RTCRtpSender | null = null;
  private screenShareAudioTrack: RTCRtpSender | null = null;

  // Webcam state
  private webcamStream: MediaStream | null = null;
  private webcamSender: RTCRtpSender | null = null;

  // Mic test
  private micTestStream: MediaStream | null = null;
  private micTestAnalyser: AnalyserNode | null = null;
  private audioContext: AudioContext | null = null;

  // Voice Activity Detection (VAD)
  private vadAnalyser: AnalyserNode | null = null;
  private vadAudioContext: AudioContext | null = null;
  private vadInterval: number | null = null;

  // Event handlers
  private eventHandlers: Partial<VoiceAdapterEvents> = {};

  // Selected devices
  private inputDeviceId: string | null = null;
  // Output device selection implemented via setSinkId API (no need to store deviceId)

  // Metrics tracking for delta calculation
  private prevStats: {
    lost: number;
    received: number;
    timestamp: number;
  } | null = null;

  // Voice join start time for timing breadcrumb
  private joinStartTime = 0;

  constructor() {
    console.log("[BrowserVoiceAdapter] Initialized");
  }

  // Lifecycle methods

  async join(channelId: string): Promise<VoiceResult<void>> {
    console.log(`[BrowserVoiceAdapter] Joining channel: ${channelId}`);

    // Clean up any stale connection (e.g., from WebSocket reconnect)
    if (this.peerConnection) {
      console.log("[BrowserVoiceAdapter] Cleaning up stale connection");
      this.cleanup();
    }

    try {
    this.joinStartTime = Date.now();
      this.channelId = channelId;
      this.setState("requesting_media");

      // Get microphone access
      const constraints: MediaStreamConstraints = {
        audio: {
          deviceId: this.inputDeviceId
            ? { exact: this.inputDeviceId }
            : undefined,
          noiseSuppression: this.noiseSuppression,
          echoCancellation: true,
          autoGainControl: true,
        },
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

      console.log(
        "[BrowserVoiceAdapter] Peer connection created, sending voice_join",
      );

      // Ensure WebSocket is connected before sending
      const { wsSend, wsStatus, wsConnect } = await import("@/lib/tauri");

      // Check WebSocket status
      let status = await wsStatus();
      console.log("[BrowserVoiceAdapter] WebSocket status:", status);

      // If disconnected, try to connect
      if (status.type === "disconnected") {
        console.log(
          "[BrowserVoiceAdapter] WebSocket disconnected, attempting to connect...",
        );
        try {
          await wsConnect();
          status = await wsStatus();
          console.log("[BrowserVoiceAdapter] WebSocket reconnected:", status);
        } catch (err) {
          console.error(
            "[BrowserVoiceAdapter] Failed to connect WebSocket:",
            err,
          );
          throw new Error(
            "Failed to connect to server. Please refresh the page.",
          );
        }
      }

      // Wait for WebSocket to be connected (with timeout)
      let attempts = 0;
      const maxAttempts = 30; // 3 seconds max
      while (attempts < maxAttempts && status.type !== "connected") {
        if (status.type === "disconnected") {
          throw new Error("WebSocket disconnected unexpectedly.");
        }
        await new Promise((resolve) => setTimeout(resolve, 100));
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

    // Clean up screen share and webcam first
    if (this.screenShareStream) {
      this.cleanupScreenShareState();
    }
    if (this.webcamStream) {
      this.cleanupWebcamState();
    }

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
    this.prevStats = null;

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

  async setNoiseSuppression(enabled: boolean): Promise<VoiceResult<void>> {
    console.log(`[BrowserVoiceAdapter] Set noise suppression: ${enabled}`);
    this.noiseSuppression = enabled;

    // Apply to current track if active
    if (this.localStream) {
      const track = this.localStream.getAudioTracks()[0];
      if (track) {
        try {
          await track.applyConstraints({
            noiseSuppression: enabled,
            echoCancellation: true, // Keep echo cancellation on
            autoGainControl: true,
          });
          console.log(
            "[BrowserVoiceAdapter] Applied constraints to active track",
          );
        } catch (err) {
          console.warn(
            "[BrowserVoiceAdapter] Failed to apply constraints, restarting stream needed:",
            err,
          );
          // If applyConstraints fails (some browsers), we might need to restart the stream
          // For now, we accept best-effort or require rejoin/device switch
        }
      }
    }

    return { ok: true, value: undefined };
  }

  // Signaling

  async handleOffer(
    channelId: string,
    sdp: string,
  ): Promise<VoiceResult<string>> {
    console.log(
      `[BrowserVoiceAdapter] Handling offer for channel: ${channelId}`,
    );

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
    candidate: string,
  ): Promise<VoiceResult<void>> {
    const startTime = performance.now();

    if (!this.peerConnection) {
      console.warn(
        "[BrowserVoiceAdapter] No peer connection for ICE candidate",
      );
      return {
        ok: false,
        error: { type: "not_connected" },
      };
    }

    if (this.channelId !== channelId) {
      console.warn(
        `[BrowserVoiceAdapter] ICE candidate for wrong channel: ${channelId}`,
      );
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
      // Parse and add ICE candidate immediately (critical for NAT traversal)
      const candidateInit = JSON.parse(candidate);
      await this.peerConnection.addIceCandidate(
        new RTCIceCandidate(candidateInit),
      );

      const elapsed = performance.now() - startTime;
      console.log(
        `[BrowserVoiceAdapter] ICE candidate added successfully (${elapsed.toFixed(2)}ms)`,
      );

      return { ok: true, value: undefined };
    } catch (err) {
      const elapsed = performance.now() - startTime;
      console.error(
        `[BrowserVoiceAdapter] Failed to add ICE candidate after ${elapsed.toFixed(2)}ms:`,
        err,
      );

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

  isNoiseSuppressionEnabled(): boolean {
    return this.noiseSuppression;
  }

  private calculateQuality(
    latency: number,
    loss: number,
    jitter: number,
  ): QualityLevel {
    // Semantic quality levels: good > warning > poor > unknown
    if (latency > 200 || loss > 3 || jitter > 50) return "poor";
    if (latency > 100 || loss > 1 || jitter > 30) return "warning";
    return "good";
  }

  async getConnectionMetrics(): Promise<ConnectionMetrics | null> {
    if (!this.peerConnection) return null;

    try {
      const stats = await this.peerConnection.getStats();
      let latency = 0;
      let jitter = 0;
      let totalLost = 0;
      let totalReceived = 0;

      stats.forEach((report) => {
        if (report.type === "candidate-pair" && report.state === "succeeded") {
          latency = (report.currentRoundTripTime ?? 0) * 1000;
        }
        if (report.type === "inbound-rtp" && report.kind === "audio") {
          totalLost += report.packetsLost ?? 0;
          totalReceived += report.packetsReceived ?? 0;
          jitter = Math.max(jitter, (report.jitter ?? 0) * 1000);
        }
      });

      // Calculate delta packet loss since last sample
      let packetLoss = 0;
      const now = Date.now();

      if (this.prevStats) {
        const deltaLost = totalLost - this.prevStats.lost;
        const deltaReceived = totalReceived - this.prevStats.received;
        const deltaTotal = deltaLost + deltaReceived;

        if (deltaTotal > 0) {
          packetLoss = (deltaLost / deltaTotal) * 100;
        }
      }

      this.prevStats = {
        lost: totalLost,
        received: totalReceived,
        timestamp: now,
      };

      return {
        latency: Math.round(latency),
        packetLoss: Math.round(packetLoss * 100) / 100,
        jitter: Math.round(jitter),
        quality: this.calculateQuality(latency, packetLoss, jitter),
        timestamp: now,
      };
    } catch (err) {
      console.warn("Failed to extract metrics:", err);
      return null;
    }
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
      this.micTestStream =
        await navigator.mediaDevices.getUserMedia(constraints);

      // Set up audio analysis for level metering
      this.audioContext = new AudioContext();
      const source = this.audioContext.createMediaStreamSource(
        this.micTestStream,
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
          audio: {
            deviceId: { exact: deviceId },
            noiseSuppression: this.noiseSuppression,
            echoCancellation: true,
            autoGainControl: true,
          },
        };
        this.localStream =
          await navigator.mediaDevices.getUserMedia(constraints);

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

    // Set output device for all remote audio elements
    // Uses setSinkId API (supported in modern browsers)
    try {
      for (const stream of this.remoteStreams.values()) {
        const audioElements = document.querySelectorAll(
          `audio[data-stream-id="${stream.id}"]`,
        );
        audioElements.forEach((audio) => {
          if ("setSinkId" in audio) {
            (audio as AudioElementWithSinkId).setSinkId(deviceId);
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

  // Screen sharing

  isScreenSharing(): boolean {
    return this.screenShareStream !== null;
  }

  /**
   * Get information about the current screen share.
   * Returns null if not sharing.
   */
  getScreenShareInfo(): { hasAudio: boolean; sourceLabel: string } | null {
    if (!this.screenShareStream) {
      return null;
    }

    const videoTrack = this.screenShareStream.getVideoTracks()[0];
    const hasAudio = this.screenShareStream.getAudioTracks().length > 0;

    // Get source label from video track (e.g., "screen:0:0", window title, etc.)
    const sourceLabel = videoTrack?.label || "Screen";

    return { hasAudio, sourceLabel };
  }

  async startScreenShare(
    options?: ScreenShareOptions,
  ): Promise<VoiceResult<void>> {
    if (!this.peerConnection) {
      return { ok: false, error: { type: "not_connected" } };
    }

    if (this.screenShareStream) {
      return {
        ok: false,
        error: { type: "unknown", message: "Already sharing screen" },
      };
    }

    try {
      // Request display media with quality constraints
      const constraints = this.getDisplayMediaConstraints(
        options?.quality ?? "medium",
      );

      const stream = await navigator.mediaDevices.getDisplayMedia({
        video: constraints.video,
        audio: options?.withAudio ?? false,
      });

      // Get the video track
      const videoTrack = stream.getVideoTracks()[0];
      if (!videoTrack) {
        stream.getTracks().forEach((t) => t.stop());
        return {
          ok: false,
          error: { type: "unknown", message: "No video track in stream" },
        };
      }

      // Listen for track ending (user clicked "Stop sharing" in browser UI)
      videoTrack.onended = () => {
        console.log("[BrowserVoiceAdapter] Screen share track ended by user");
        this.handleScreenShareEnded();
      };

      // Add video track to peer connection
      this.screenShareTrack = this.peerConnection.addTrack(videoTrack, stream);

      // If audio track present, add it too
      const audioTrack = stream.getAudioTracks()[0];
      if (audioTrack) {
        this.screenShareAudioTrack = this.peerConnection.addTrack(
          audioTrack,
          stream,
        );
      }

      this.screenShareStream = stream;

      console.log("[BrowserVoiceAdapter] Screen share started", {
        hasAudio: !!audioTrack,
        quality: options?.quality ?? "medium",
      });

      return { ok: true, value: undefined };
    } catch (err) {
      console.error("[BrowserVoiceAdapter] Failed to start screen share:", err);

      // Handle specific errors with actionable messages
      if (err instanceof DOMException) {
        switch (err.name) {
          case "NotAllowedError":
            return {
              ok: false,
              error: {
                type: "permission_denied",
                message:
                  "Screen share permission denied. Please allow screen sharing in your browser.",
              },
            };
          case "AbortError":
            return {
              ok: false,
              error: {
                type: "cancelled",
                message: "Screen share cancelled",
              },
            };
          case "NotFoundError":
            return {
              ok: false,
              error: {
                type: "not_found",
                message: "No screen or window found to share",
              },
            };
          case "NotReadableError":
            return {
              ok: false,
              error: {
                type: "hardware_error",
                message:
                  "Could not access screen. Another app may be blocking screen capture.",
              },
            };
          case "OverconstrainedError":
            return {
              ok: false,
              error: {
                type: "constraint_error",
                message:
                  "Screen share quality settings not supported by your system",
              },
            };
          default:
            return {
              ok: false,
              error: {
                type: "unknown",
                message: `Screen share failed: ${err.message}`,
              },
            };
        }
      }

      // Handle other error types
      if (err instanceof Error) {
        console.error("[BrowserVoiceAdapter] Screen share error details:", {
          name: err.name,
          message: err.message,
          stack: err.stack,
        });
        return {
          ok: false,
          error: {
            type: "unknown",
            message: `Screen share failed: ${err.message}. Try closing other video applications.`,
          },
        };
      }

      return {
        ok: false,
        error: { type: "unknown", message: "Screen share failed unexpectedly" },
      };
    }
  }

  async stopScreenShare(): Promise<VoiceResult<void>> {
    if (!this.screenShareStream) {
      return {
        ok: false,
        error: { type: "unknown", message: "Not sharing screen" },
      };
    }

    try {
      this.cleanupScreenShareState();
      console.log("[BrowserVoiceAdapter] Screen share stopped");
      return { ok: true, value: undefined };
    } catch (err) {
      console.error("[BrowserVoiceAdapter] Failed to stop screen share:", err);
      return { ok: false, error: { type: "unknown", message: String(err) } };
    }
  }

  // Webcam

  isWebcamActive(): boolean {
    return this.webcamStream !== null;
  }

  async startWebcam(options?: WebcamOptions): Promise<VoiceResult<void>> {
    if (!this.peerConnection) {
      return { ok: false, error: { type: "not_connected" } };
    }

    if (this.webcamStream) {
      return {
        ok: false,
        error: { type: "unknown", message: "Webcam already active" },
      };
    }

    try {
      const constraints: MediaStreamConstraints = {
        video: {
          deviceId: options?.deviceId ? { exact: options.deviceId } : undefined,
          width: { ideal: 640 },
          height: { ideal: 480 },
          frameRate: { ideal: 30 },
        },
      };

      const stream = await navigator.mediaDevices.getUserMedia(constraints);
      const videoTrack = stream.getVideoTracks()[0];
      if (!videoTrack) {
        stream.getTracks().forEach((t) => t.stop());
        return {
          ok: false,
          error: {
            type: "unknown",
            message: "No video track in webcam stream",
          },
        };
      }

      // Listen for track ending (e.g., user revokes camera permission)
      videoTrack.onended = () => {
        console.log("[BrowserVoiceAdapter] Webcam track ended by user/system");
        this.cleanupWebcamState();
      };

      // Add video track to peer connection
      this.webcamSender = this.peerConnection.addTrack(videoTrack, stream);
      this.webcamStream = stream;

      console.log("[BrowserVoiceAdapter] Webcam started");
      return { ok: true, value: undefined };
    } catch (err) {
      console.error("[BrowserVoiceAdapter] Failed to start webcam:", err);
      return { ok: false, error: this.mapMediaError(err) };
    }
  }

  async stopWebcam(): Promise<VoiceResult<void>> {
    if (!this.webcamStream) {
      return {
        ok: false,
        error: { type: "unknown", message: "Webcam not active" },
      };
    }

    try {
      this.cleanupWebcamState();
      console.log("[BrowserVoiceAdapter] Webcam stopped");
      return { ok: true, value: undefined };
    } catch (err) {
      console.error("[BrowserVoiceAdapter] Failed to stop webcam:", err);
      return { ok: false, error: { type: "unknown", message: String(err) } };
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

  private getDisplayMediaConstraints(
    quality: ScreenShareQuality,
  ): DisplayMediaStreamOptions {
    const qualitySettings = {
      low: { width: 854, height: 480, frameRate: 15 },
      medium: { width: 1280, height: 720, frameRate: 30 },
      high: { width: 1920, height: 1080, frameRate: 30 },
      premium: { width: 1920, height: 1080, frameRate: 60 },
    };

    const settings = qualitySettings[quality];

    return {
      video: {
        cursor: "always",
        width: { ideal: settings.width, max: settings.width },
        height: { ideal: settings.height, max: settings.height },
        frameRate: { ideal: settings.frameRate, max: settings.frameRate },
      } as MediaTrackConstraints,
    };
  }

  private handleScreenShareEnded(): void {
    this.cleanupScreenShareState();
    // Notify via event handler - need to get user ID
    // For browser, we don't easily have our own user ID here, so pass empty
    this.eventHandlers.onScreenShareStopped?.("", "user_stopped");
  }

  private cleanupScreenShareState(): void {
    // Remove tracks from peer connection
    if (this.peerConnection) {
      if (this.screenShareAudioTrack) {
        this.peerConnection.removeTrack(this.screenShareAudioTrack);
      }
      if (this.screenShareTrack) {
        this.peerConnection.removeTrack(this.screenShareTrack);
      }
    }

    // Stop the stream tracks
    if (this.screenShareStream) {
      this.screenShareStream.getTracks().forEach((track) => track.stop());
    }

    // Clear state
    this.screenShareStream = null;
    this.screenShareTrack = null;
    this.screenShareAudioTrack = null;
  }

  private cleanupWebcamState(): void {
    // Remove track from peer connection
    if (this.peerConnection && this.webcamSender) {
      this.peerConnection.removeTrack(this.webcamSender);
    }

    // Stop the stream tracks
    if (this.webcamStream) {
      this.webcamStream.getTracks().forEach((track) => track.stop());
    }

    // Clear state
    this.webcamStream = null;
    this.webcamSender = null;
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
          const elapsed = Date.now() - this.joinStartTime;
          Sentry.addBreadcrumb({ category: "voice", message: "voice_connected", data: { channel_id: this.channelId ?? "", duration_ms: elapsed }, level: "info" });
          this.startVAD(); // Start Voice Activity Detection
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

    // Server-driven renegotiation: suppress browser-initiated negotiation
    this.peerConnection.onnegotiationneeded = () => {
      console.log(
        "[BrowserVoiceAdapter] Negotiation needed (server-driven, ignoring)",
      );
    };

    // Remote track handler
    this.peerConnection.ontrack = (event) => {
      const track = event.track;
      const stream = event.streams[0];

      // Parse stream ID format: "{userId}:{sourceType}" (e.g. "uuid:Webcam", "uuid:ScreenVideo")
      const [userId, sourceType] = stream.id.split(":");

      console.log(
        `[BrowserVoiceAdapter] Remote ${track.kind} track received, source: ${sourceType}, from: ${userId}`,
      );

      if (track.kind === "video") {
        if (sourceType === "Webcam") {
          // Webcam video track
          console.log("[BrowserVoiceAdapter] Webcam video track from:", userId);
          this.eventHandlers.onWebcamTrack?.(userId, track);

          track.onended = () => {
            console.log(
              "[BrowserVoiceAdapter] Webcam track ended for:",
              userId,
            );
            this.eventHandlers.onWebcamTrackRemoved?.(userId);
          };
        } else {
          // Screen share video track (ScreenVideo or fallback)
          console.log(
            "[BrowserVoiceAdapter] Screen share video track from:",
            userId,
          );
          this.eventHandlers.onScreenShareTrack?.(userId, track);

          track.onended = () => {
            console.log("[BrowserVoiceAdapter] Screen share track ended");
            this.eventHandlers.onScreenShareTrackRemoved?.(userId);
          };
        }
      } else {
        // Audio track = voice (Microphone) or screen audio (ScreenAudio)
        this.remoteStreams.set(userId, stream);

        const remoteTrack: RemoteTrack = {
          trackId: track.id,
          userId,
          stream,
          muted: false,
        };

        this.eventHandlers.onRemoteTrack?.(remoteTrack);

        track.onended = () => {
          console.log("[BrowserVoiceAdapter] Remote audio track ended");
          this.remoteStreams.delete(userId);
          this.eventHandlers.onRemoteTrackRemoved?.(userId);
        };
      }
    };
  }

  private startVAD() {
    if (!this.localStream) return;

    console.log("[BrowserVoiceAdapter] Starting VAD monitoring");

    try {
      // Create audio context for VAD
      this.vadAudioContext = new AudioContext();
      const source = this.vadAudioContext.createMediaStreamSource(
        this.localStream,
      );
      this.vadAnalyser = this.vadAudioContext.createAnalyser();
      this.vadAnalyser.fftSize = 256;
      source.connect(this.vadAnalyser);

      // Monitor audio level every 100ms
      this.vadInterval = window.setInterval(() => {
        if (!this.vadAnalyser || this.muted) {
          // Don't trigger speaking when muted
          this.eventHandlers.onSpeakingChange?.(false);
          return;
        }

        const dataArray = new Uint8Array(this.vadAnalyser.frequencyBinCount);
        this.vadAnalyser.getByteFrequencyData(dataArray);

        // Calculate RMS volume
        const sum = dataArray.reduce((acc, val) => acc + val, 0);
        const average = sum / dataArray.length;

        // Normalize to 0-100
        const level = Math.min(100, Math.round((average / 255) * 100 * 2));

        // Speaking threshold: 20%
        const isSpeaking = level > 20;
        this.eventHandlers.onSpeakingChange?.(isSpeaking);
      }, 100);
    } catch (err) {
      console.error("[BrowserVoiceAdapter] Failed to start VAD:", err);
    }
  }

  private stopVAD() {
    console.log("[BrowserVoiceAdapter] Stopping VAD monitoring");

    if (this.vadInterval) {
      clearInterval(this.vadInterval);
      this.vadInterval = null;
    }

    if (this.vadAudioContext) {
      this.vadAudioContext.close();
      this.vadAudioContext = null;
    }

    this.vadAnalyser = null;
  }

  private cleanup() {
    // Stop VAD
    this.stopVAD();

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
