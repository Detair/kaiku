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

/** Bitrate for the highest simulcast layer, keyed by screen-share quality. */
const QUALITY_BITRATES: Record<ScreenShareQuality, number> = {
  low: 1_000_000, // 480p
  medium: 2_500_000, // 720p
  high: 4_000_000, // 1080p30
  premium: 6_000_000, // 1080p60
};


/** Build 3-layer simulcast encodings (high / medium / low). */
function simulcastEncodings(
  highBitrate: number,
  highFramerate: number = 30,
): RTCRtpEncodingParameters[] {
  return [
    { rid: "h", maxBitrate: highBitrate, scaleResolutionDownBy: 1.0, maxFramerate: highFramerate },
    { rid: "m", maxBitrate: 800_000, scaleResolutionDownBy: 2.0, maxFramerate: Math.min(24, highFramerate) },
    { rid: "l", maxBitrate: 200_000, scaleResolutionDownBy: 4.0, maxFramerate: 15 },
  ];
}

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

  // WebRTC — dual PeerConnection model
  private publisherPC: RTCPeerConnection | null = null;
  private subscriberPC: RTCPeerConnection | null = null;
  private localStream: MediaStream | null = null;
  private remoteStreams = new Map<string, MediaStream>();

  // Screen share state — keyed by streamId for multi-stream support
  private screenShares: Map<string, {
    stream: MediaStream;
    videoSender: RTCRtpSender;
    audioSender: RTCRtpSender | null;
  }> = new Map();

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

  // Negotiation lock to prevent concurrent offers on the publisher PC
  private isNegotiating = false;
  private pendingNegotiation = false;

  constructor() {
    console.log("[BrowserVoiceAdapter] Initialized");
  }

  // Lifecycle methods

  async join(channelId: string): Promise<VoiceResult<void>> {
    console.log(`[BrowserVoiceAdapter] Joining channel: ${channelId}`);

    // Clean up any stale connection (e.g., from WebSocket reconnect)
    if (this.publisherPC || this.subscriberPC) {
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

      // Fetch ICE servers (STUN + TURN) from the API
      const config = await this.fetchIceConfig();

      // Create dual PeerConnections
      this.publisherPC = new RTCPeerConnection(config);
      this.subscriberPC = new RTCPeerConnection(config);
      this.setupPublisherPC(channelId);
      this.setupSubscriberPC(channelId);

      console.log(
        "[BrowserVoiceAdapter] Dual PeerConnections created, sending voice_join",
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

      // Add mic track to publisherPC — this triggers onnegotiationneeded
      // which creates and sends the publisher offer automatically
      this.localStream.getTracks().forEach((track) => {
        this.publisherPC!.addTrack(track, this.localStream!);
      });

      console.log("[BrowserVoiceAdapter] Mic tracks added, publisher offer will be sent via onnegotiationneeded");

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

    // Clean up all screen shares and webcam first
    if (this.screenShares.size > 0) {
      await this.stopAllScreenShares();
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

  // Signaling — dual PeerConnection model

  async handlePublisherAnswer(
    channelId: string,
    sdp: string,
  ): Promise<VoiceResult<void>> {
    console.log(
      `[BrowserVoiceAdapter] Handling publisher answer for channel: ${channelId}`,
    );

    if (!this.publisherPC) {
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
          message: `Received publisher answer for wrong channel: ${channelId}`,
        },
      };
    }

    try {
      await this.publisherPC.setRemoteDescription(
        new RTCSessionDescription({ type: "answer", sdp }),
      );
      console.log("[BrowserVoiceAdapter] Publisher answer applied");

      // Release negotiation lock
      this.isNegotiating = false;

      // Drain pending negotiation
      if (this.pendingNegotiation) {
        this.pendingNegotiation = false;
        this.publisherPC.dispatchEvent(new Event("negotiationneeded"));
      }

      return { ok: true, value: undefined };
    } catch (err) {
      this.isNegotiating = false;
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

  async handleSubscriberOffer(
    channelId: string,
    sdp: string,
  ): Promise<VoiceResult<string>> {
    console.log(
      `[BrowserVoiceAdapter] Handling subscriber offer for channel: ${channelId}`,
    );

    if (!this.subscriberPC) {
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
          message: `Received subscriber offer for wrong channel: ${channelId}`,
        },
      };
    }

    try {
      await this.subscriberPC.setRemoteDescription(
        new RTCSessionDescription({ type: "offer", sdp }),
      );
      const answer = await this.subscriberPC.createAnswer();
      await this.subscriberPC.setLocalDescription(answer);

      console.log("[BrowserVoiceAdapter] Subscriber answer created");

      const answerSdp = answer.sdp;
      if (!answerSdp) {
        return {
          ok: false,
          error: {
            type: "connection_failed" as const,
            reason: "Browser returned empty SDP answer",
            retriable: true,
          },
        };
      }
      return { ok: true, value: answerSdp };
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
    pcType: string = "publisher",
  ): Promise<VoiceResult<void>> {
    const startTime = performance.now();

    const pc = pcType === "subscriber" ? this.subscriberPC : this.publisherPC;

    if (!pc) {
      console.warn(
        `[BrowserVoiceAdapter] No ${pcType} peer connection for ICE candidate`,
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
      await pc.addIceCandidate(new RTCIceCandidate(candidateInit));

      const elapsed = performance.now() - startTime;
      console.log(
        `[BrowserVoiceAdapter] ICE candidate (${pcType}) added successfully (${elapsed.toFixed(2)}ms)`,
      );

      return { ok: true, value: undefined };
    } catch (err) {
      const elapsed = performance.now() - startTime;
      console.error(
        `[BrowserVoiceAdapter] Failed to add ICE candidate (${pcType}) after ${elapsed.toFixed(2)}ms:`,
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
    if (!this.publisherPC) return null;

    try {
      const stats = await this.publisherPC.getStats();
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
    if (this.localStream && this.publisherPC) {
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

        // Replace tracks in publisher peer connection
        const sender = this.publisherPC
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
    return this.screenShares.size > 0;
  }

  /**
   * Get information about a specific screen share (or the first one if no streamId).
   * Returns null if not sharing.
   */
  getScreenShareInfo(streamId?: string): { streamId: string; hasAudio: boolean; sourceLabel: string } | null {
    if (this.screenShares.size === 0) {
      return null;
    }

    if (streamId) {
      const entry = this.screenShares.get(streamId);
      if (!entry) return null;

      const videoTrack = entry.stream.getVideoTracks()[0];
      const hasAudio = entry.stream.getAudioTracks().length > 0;
      const sourceLabel = videoTrack?.label || "Screen";

      return { streamId, hasAudio, sourceLabel };
    }

    // No streamId specified — return first entry
    const [firstId, firstEntry] = this.screenShares.entries().next().value!;
    const videoTrack = firstEntry.stream.getVideoTracks()[0];
    const hasAudio = firstEntry.stream.getAudioTracks().length > 0;
    const sourceLabel = videoTrack?.label || "Screen";

    return { streamId: firstId, hasAudio, sourceLabel };
  }

  /**
   * Get the video track for a local screen share.
   */
  getScreenShareTrack(streamId: string): MediaStreamTrack | null {
    const entry = this.screenShares.get(streamId);
    return entry?.stream.getVideoTracks()[0] ?? null;
  }

  /**
   * Get info for all active screen shares.
   */
  getAllScreenShareInfo(): { streamId: string; hasAudio: boolean; sourceLabel: string }[] {
    const result: { streamId: string; hasAudio: boolean; sourceLabel: string }[] = [];
    for (const [id, entry] of this.screenShares) {
      const videoTrack = entry.stream.getVideoTracks()[0];
      const hasAudio = entry.stream.getAudioTracks().length > 0;
      const sourceLabel = videoTrack?.label || "Screen";
      result.push({ streamId: id, hasAudio, sourceLabel });
    }
    return result;
  }

  async startScreenShare(
    options?: ScreenShareOptions,
  ): Promise<VoiceResult<void>> {
    if (!this.publisherPC) {
      return { ok: false, error: { type: "not_connected" } };
    }

    try {
      const streamId = options?.streamId ?? crypto.randomUUID();

      // Capture screen with quality constraints
      const constraints = this.getDisplayMediaConstraints(
        options?.quality ?? "medium",
      );
      const stream = await navigator.mediaDevices.getDisplayMedia({
        video: constraints.video,
        audio: options?.withAudio ?? false,
      });

      // Add video track to publisher PC
      const videoTrack = stream.getVideoTracks()[0];
      const videoSender = this.publisherPC.addTrack(videoTrack, stream);

      // Add audio track if present
      let audioSender: RTCRtpSender | null = null;
      const audioTrack = stream.getAudioTracks()[0];
      if (audioTrack) {
        audioSender = this.publisherPC.addTrack(audioTrack, stream);
      }

      // onnegotiationneeded fires automatically -> creates offer -> server answers -> RTP flows

      // Handle user stopping share via browser UI
      videoTrack.onended = () => {
        console.log("[BrowserVoiceAdapter] Screen share track ended by user, streamId:", streamId);
        this.stopScreenShare(streamId);
      };

      // Store for cleanup
      this.screenShares.set(streamId, { stream, videoSender, audioSender });

      console.log("[BrowserVoiceAdapter] Screen share started, streamId:", streamId);

      return { ok: true, value: undefined };
    } catch (err) {
      console.error("[BrowserVoiceAdapter] Failed to start screen share:", err);
      return { ok: false, error: this.mapScreenShareError(err) };
    }
  }

  async stopScreenShare(streamId?: string): Promise<VoiceResult<void>> {
    if (!this.publisherPC) {
      return { ok: false, error: { type: "not_connected" } };
    }

    const targetId = streamId || this.screenShares.keys().next().value;
    if (!targetId) {
      return {
        ok: false,
        error: { type: "unknown", message: "No active screen share" },
      };
    }

    const share = this.screenShares.get(targetId);
    if (!share) {
      return {
        ok: false,
        error: { type: "unknown", message: `Screen share ${targetId} not found` },
      };
    }

    // Remove tracks from publisher PC
    this.publisherPC.removeTrack(share.videoSender);
    if (share.audioSender) {
      this.publisherPC.removeTrack(share.audioSender);
    }

    // Stop media tracks
    share.stream.getTracks().forEach((t) => t.stop());

    // onnegotiationneeded fires automatically -> renegotiates

    this.screenShares.delete(targetId);

    // Notify listeners
    this.eventHandlers.onScreenShareStopped?.("", targetId, "user_stopped");

    console.log("[BrowserVoiceAdapter] Screen share stopped:", targetId);

    return { ok: true, value: undefined };
  }

  async stopAllScreenShares(): Promise<VoiceResult<void>> {
    if (this.screenShares.size === 0) {
      return { ok: true, value: undefined };
    }

    // Collect IDs first to avoid mutating map during iteration
    const streamIds = [...this.screenShares.keys()];
    for (const id of streamIds) {
      await this.stopScreenShare(id);
    }

    console.log("[BrowserVoiceAdapter] All screen shares stopped");
    return { ok: true, value: undefined };
  }

  // Webcam

  isWebcamActive(): boolean {
    return this.webcamStream !== null;
  }

  async startWebcam(options?: WebcamOptions): Promise<VoiceResult<void>> {
    if (!this.publisherPC) {
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

      // Add video track to publisher peer connection with simulcast encodings
      const webcamBitrate = QUALITY_BITRATES[options?.quality ?? "medium"];
      const webcamTransceiver = this.publisherPC.addTransceiver(videoTrack, {
        direction: "sendonly",
        streams: [stream],
        sendEncodings: simulcastEncodings(webcamBitrate),
      });
      this.webcamSender = webcamTransceiver.sender;
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

  private async fetchIceConfig(): Promise<RTCConfiguration> {
    const fallback: RTCConfiguration = {
      iceServers: [{ urls: "stun:stun.l.google.com:19302" }],
    };
    try {
      const { getServerUrl, getAccessToken } = await import("@/lib/tauri");
      const serverUrl = getServerUrl();
      const token = getAccessToken();
      if (!token) {
        console.warn("[BrowserVoiceAdapter] No access token, skipping ICE server fetch");
        return fallback;
      }
      const res = await fetch(`${serverUrl}/api/voice/ice-servers`, {
        headers: { Authorization: `Bearer ${token}` },
      });
      if (!res.ok) {
        console.error(
          `[BrowserVoiceAdapter] ICE server fetch failed: HTTP ${res.status}. ` +
          `TURN unavailable — users behind restrictive NATs may not connect.`,
        );
        Sentry.addBreadcrumb({
          category: "voice",
          message: `ICE config fetch failed: ${res.status}`,
          level: "error",
        });
        return fallback;
      }
      const data = await res.json();
      const iceServers: RTCIceServer[] = (data.ice_servers ?? []).map(
        (s: { urls: string[]; username?: string; credential?: string }) => ({
          urls: s.urls,
          ...(s.username && { username: s.username }),
          ...(s.credential && { credential: s.credential }),
        }),
      );
      if (iceServers.length > 0) {
        const hasTurn = iceServers.some((s) =>
          (Array.isArray(s.urls) ? s.urls : [s.urls]).some((u) => u.startsWith("turn:")),
        );
        console.log(`[BrowserVoiceAdapter] ICE config: ${iceServers.length} servers, TURN=${hasTurn}`);
        return { iceServers };
      }
      return fallback;
    } catch (err) {
      console.error("[BrowserVoiceAdapter] ICE server fetch exception, using fallback STUN", err);
      Sentry.addBreadcrumb({
        category: "voice",
        message: "ICE config fetch exception",
        data: { error: err instanceof Error ? err.message : String(err) },
        level: "error",
      });
      return fallback;
    }
  }

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

  /**
   * Map screen share errors to typed VoiceError with actionable messages.
   */
  private mapScreenShareError(err: unknown): VoiceError {
    if (err instanceof DOMException) {
      switch (err.name) {
        case "NotAllowedError":
          return {
            type: "permission_denied",
            message: "Screen share permission denied. Please allow screen sharing in your browser.",
          };
        case "AbortError":
          return { type: "cancelled", message: "Screen share cancelled" };
        case "NotFoundError":
          return { type: "not_found", message: "No screen or window found to share" };
        case "NotReadableError":
          return {
            type: "hardware_error",
            message: "Could not access screen. Another app may be blocking screen capture.",
          };
        case "OverconstrainedError":
          return {
            type: "constraint_error",
            message: "Screen share quality settings not supported by your system",
          };
        default:
          return { type: "unknown", message: `Screen share failed: ${err.message}` };
      }
    }

    return {
      type: "unknown",
      message: err instanceof Error ? err.message : "Screen share failed unexpectedly",
    };
  }

  private cleanupWebcamState(): void {
    // Remove track from publisher peer connection
    if (this.publisherPC && this.webcamSender) {
      this.publisherPC.removeTrack(this.webcamSender);
    }

    // Stop the stream tracks
    if (this.webcamStream) {
      this.webcamStream.getTracks().forEach((track) => track.stop());
    }

    // Clear state
    this.webcamStream = null;
    this.webcamSender = null;
  }

  private setupPublisherPC(channelId: string) {
    if (!this.publisherPC) return;

    // Client-initiated negotiation: create offer and send to server
    // Uses a negotiation lock to prevent concurrent offers when multiple
    // tracks are added in quick succession (e.g., mic + screen share).
    this.publisherPC.onnegotiationneeded = async () => {
      if (!this.publisherPC) return;

      if (this.isNegotiating) {
        this.pendingNegotiation = true;
        return;
      }

      this.isNegotiating = true;
      try {
        const offer = await this.publisherPC.createOffer();
        await this.publisherPC.setLocalDescription(offer);

        const { wsSend } = await import("@/lib/tauri");
        wsSend({
          type: "voice_publisher_offer",
          channel_id: channelId,
          sdp: offer.sdp!,
        });
      } catch (err) {
        console.error("[BrowserVoiceAdapter] Publisher negotiation failed:", err);
        this.isNegotiating = false;
      }
    };

    // ICE candidate handler for publisher
    this.publisherPC.onicecandidate = (event) => {
      if (event.candidate) {
        const candidateJson = JSON.stringify(event.candidate.toJSON());
        // Send ICE candidate with pc_type via WS
        import("@/lib/tauri")
          .then(({ wsSend }) =>
            wsSend({
              type: "voice_ice_candidate",
              channel_id: channelId,
              candidate: candidateJson,
              pc_type: "publisher",
            })
          )
          .catch((err) => {
            console.error("[BrowserVoiceAdapter] Failed to send publisher ICE candidate:", err);
          });
      }
    };

    // Connection state change — publisher PC determines connected state
    this.publisherPC.onconnectionstatechange = () => {
      const state = this.publisherPC!.connectionState;
      console.log(`[BrowserVoiceAdapter] Publisher connection state: ${state}`);

      switch (state) {
        case "connected":
          this.setState("connected");
          {
            const elapsed = Date.now() - this.joinStartTime;
            Sentry.addBreadcrumb({ category: "voice", message: "voice_connected", data: { channel_id: this.channelId ?? "", duration_ms: elapsed }, level: "info" });
          }
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
            reason: "Publisher peer connection failed",
            retriable: true,
          });
          break;
        case "connecting":
        case "new":
          this.setState("connecting");
          break;
      }
    };
  }

  private setupSubscriberPC(channelId: string) {
    if (!this.subscriberPC) return;

    // Remote track handler — all remote tracks arrive on subscriberPC
    this.subscriberPC.ontrack = (event) => {
      const track = event.track;
      const stream = event.streams[0];

      if (!stream) {
        console.warn(`[BrowserVoiceAdapter] Remote ${track.kind} track received WITHOUT stream association — cannot identify source`);
        return;
      }

      // Parse stream ID format: "{userId}:{sourceType}" using Display format
      // (e.g. "uuid:webcam", "uuid:screen_video:stream_uuid", "uuid:microphone")
      // Split on first colon only to get [userId, sourceType]
      const colonIdx = stream.id.indexOf(":");
      const userId = colonIdx > 0 ? stream.id.slice(0, colonIdx) : stream.id;
      const sourceType = colonIdx > 0 ? stream.id.slice(colonIdx + 1) : "";

      console.log(
        `[BrowserVoiceAdapter] Remote ${track.kind} track received on subscriberPC, source: ${sourceType}, from: ${userId}, streamId: ${stream.id}`,
      );

      if (track.kind === "video") {
        if (sourceType === "webcam") {
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
          // Screen share video track — parse stream_id from "screen_video:uuid"
          const streamId = this.parseScreenShareStreamId(sourceType);
          console.log(
            "[BrowserVoiceAdapter] Screen share video track from:",
            userId,
            "streamId:",
            streamId,
          );
          this.eventHandlers.onScreenShareTrack?.(userId, streamId, track);

          track.onended = () => {
            console.log("[BrowserVoiceAdapter] Screen share track ended, streamId:", streamId);
            this.eventHandlers.onScreenShareTrackRemoved?.(userId, streamId);
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

    // ICE candidate handler for subscriber
    this.subscriberPC.onicecandidate = (event) => {
      if (event.candidate) {
        const candidateJson = JSON.stringify(event.candidate.toJSON());
        import("@/lib/tauri")
          .then(({ wsSend }) =>
            wsSend({
              type: "voice_ice_candidate",
              channel_id: channelId,
              candidate: candidateJson,
              pc_type: "subscriber",
            })
          )
          .catch((err) => {
            console.error("[BrowserVoiceAdapter] Failed to send subscriber ICE candidate:", err);
          });
      }
    };

    // Subscriber connection state (log only, publisher determines main state)
    this.subscriberPC.onconnectionstatechange = () => {
      const state = this.subscriberPC?.connectionState;
      console.log(`[BrowserVoiceAdapter] Subscriber connection state: ${state}`);

      if (state === "failed") {
        this.eventHandlers.onError?.({
          type: "connection_failed",
          reason: "Subscriber connection failed — you may not hear other participants",
          retriable: true,
        });
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

    // Stop all screen shares (stop media tracks before closing peer connections)
    for (const entry of this.screenShares.values()) {
      entry.stream.getTracks().forEach((track) => track.stop());
    }
    this.screenShares.clear();

    // Stop local stream
    if (this.localStream) {
      this.localStream.getTracks().forEach((track) => track.stop());
      this.localStream = null;
    }

    // Close both peer connections
    if (this.publisherPC) {
      this.publisherPC.close();
      this.publisherPC = null;
    }
    if (this.subscriberPC) {
      this.subscriberPC.close();
      this.subscriberPC = null;
    }

    // Clear remote streams
    this.remoteStreams.clear();

    // Reset negotiation lock
    this.isNegotiating = false;
    this.pendingNegotiation = false;

    // Stop mic test if running
    this.stopMicTest();
  }

  /**
   * Parse stream_id from a TrackSource Display string like "screen_video:uuid".
   * Falls back to "unknown" if the format doesn't match.
   */
  private parseScreenShareStreamId(sourceType: string): string {
    // Format: "screen_video:01234567-89ab-cdef-0123-456789abcdef"
    const match = sourceType.match(/^screen_(?:video|audio):(.+)$/);
    return match?.[1] ?? "unknown";
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
