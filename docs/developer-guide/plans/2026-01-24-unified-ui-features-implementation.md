# Unified UI Features Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement four unified UI features: voice quality indicators, user presence/status, message reactions with emoji picker, and channel categories.

**Architecture:** Shared design system (StatusIndicator with accessibility shapes, display preferences) used across all features. Server handles persistence and real-time broadcasting. Client uses Solid.js stores and reactive components.

**Tech Stack:** Rust (axum, sqlx), TypeScript (Solid.js), PostgreSQL, WebSocket, Twemoji

---

## Phase 1: Shared Design System

### Task 1.1: Upgrade StatusIndicator with Accessibility Shapes

**Files:**
- Modify: `client/src/components/ui/StatusIndicator.tsx`
- Modify: `client/src/lib/types.ts`

**Step 1: Update types for quality and status**

Add to `client/src/lib/types.ts` after line 9:

```typescript
export type QualityLevel = 'good' | 'warning' | 'poor' | 'unknown';

export type StatusShape = 'circle' | 'triangle' | 'hexagon' | 'empty-circle';

export const STATUS_SHAPES: Record<QualityLevel, StatusShape> = {
  good: 'circle',
  warning: 'triangle',
  poor: 'hexagon',
  unknown: 'empty-circle',
};

export const STATUS_COLORS = {
  good: '#23a55a',
  warning: '#f0b232',
  poor: '#f23f43',
  unknown: '#80848e',
  streaming: '#593695',
} as const;
```

**Step 2: Rewrite StatusIndicator with SVG shapes**

Replace `client/src/components/ui/StatusIndicator.tsx`:

```typescript
import { Component, Show } from "solid-js";
import type { UserStatus, QualityLevel, StatusShape } from "@/lib/types";
import { STATUS_COLORS } from "@/lib/types";

interface StatusIndicatorProps {
  /** For user status: online, away, busy, offline */
  status?: UserStatus;
  /** For quality indicators: good, warning, poor, unknown */
  quality?: QualityLevel;
  /** Override shape explicitly */
  shape?: StatusShape;
  /** Size variant */
  size?: "xs" | "sm" | "md" | "lg";
  /** Show as overlay on avatar (absolute positioned) */
  overlay?: boolean;
  /** Optional text to show next to indicator (e.g., "42ms") */
  text?: string;
}

const sizeMap = {
  xs: 8,
  sm: 10,
  md: 12,
  lg: 14,
};

function getShapeForStatus(status: UserStatus): StatusShape {
  switch (status) {
    case "online": return "circle";
    case "away": return "triangle";
    case "busy": return "hexagon";
    case "offline": return "empty-circle";
  }
}

function getShapeForQuality(quality: QualityLevel): StatusShape {
  switch (quality) {
    case "good": return "circle";
    case "warning": return "triangle";
    case "poor": return "hexagon";
    case "unknown": return "empty-circle";
  }
}

function getColorForStatus(status: UserStatus): string {
  switch (status) {
    case "online": return STATUS_COLORS.good;
    case "away": return STATUS_COLORS.warning;
    case "busy": return STATUS_COLORS.poor;
    case "offline": return STATUS_COLORS.unknown;
  }
}

function getColorForQuality(quality: QualityLevel): string {
  return STATUS_COLORS[quality];
}

const StatusIndicator: Component<StatusIndicatorProps> = (props) => {
  const size = () => sizeMap[props.size ?? "md"];

  const shape = (): StatusShape => {
    if (props.shape) return props.shape;
    if (props.quality) return getShapeForQuality(props.quality);
    if (props.status) return getShapeForStatus(props.status);
    return "circle";
  };

  const color = (): string => {
    if (props.quality) return getColorForQuality(props.quality);
    if (props.status) return getColorForStatus(props.status);
    return STATUS_COLORS.unknown;
  };

  const renderShape = () => {
    const s = size();
    const c = color();
    const sh = shape();

    switch (sh) {
      case "circle":
        return (
          <svg width={s} height={s} viewBox="0 0 12 12">
            <circle cx="6" cy="6" r="5" fill={c} />
          </svg>
        );
      case "triangle":
        return (
          <svg width={s} height={s} viewBox="0 0 12 12">
            <polygon points="6,1 11,10 1,10" fill={c} />
          </svg>
        );
      case "hexagon":
        return (
          <svg width={s} height={s} viewBox="0 0 12 12">
            <polygon points="6,1 10.5,3.5 10.5,8.5 6,11 1.5,8.5 1.5,3.5" fill={c} />
          </svg>
        );
      case "empty-circle":
        return (
          <svg width={s} height={s} viewBox="0 0 12 12">
            <circle cx="6" cy="6" r="4" fill="none" stroke={c} stroke-width="2" />
          </svg>
        );
    }
  };

  const positionClass = () => props.overlay ? "absolute -bottom-0.5 -right-0.5" : "";

  return (
    <span class={`inline-flex items-center gap-1 ${positionClass()}`}>
      {renderShape()}
      <Show when={props.text}>
        <span class="text-xs" style={{ color: color() }}>{props.text}</span>
      </Show>
    </span>
  );
};

export default StatusIndicator;
```

**Step 3: Run type check**

Run: `cd client && bun run build`
Expected: Build succeeds

**Step 4: Commit**

```bash
git add client/src/components/ui/StatusIndicator.tsx client/src/lib/types.ts
git commit -m "feat(ui): upgrade StatusIndicator with accessibility shapes

- Add circle (good), triangle (warning), hexagon (poor) shapes
- Support both status (user) and quality (voice) modes
- Add optional text display for latency numbers
- SVG-based for crisp rendering at all sizes"
```

---

### Task 1.2: Add Display Preferences to Server-Synced Preferences

**Files:**
- Modify: `server/src/api/preferences.rs` (or create if needed)
- Modify: `client/src/stores/preferences.ts`
- Modify: `client/src/lib/types.ts`

**Step 1: Add display preferences types**

Add to `client/src/lib/types.ts`:

```typescript
export type DisplayMode = 'dense' | 'minimal' | 'discord';
export type ReactionStyle = 'bar' | 'compact';

export interface DisplayPreferences {
  indicatorMode: DisplayMode;
  showLatencyNumbers: boolean;
  reactionStyle: ReactionStyle;
  idleTimeoutMinutes: number;
}

export const DEFAULT_DISPLAY_PREFERENCES: DisplayPreferences = {
  indicatorMode: 'dense',
  showLatencyNumbers: true,
  reactionStyle: 'bar',
  idleTimeoutMinutes: 5,
};
```

**Step 2: Check existing preferences store and extend**

Read `client/src/stores/preferences.ts` and add display preferences integration.

**Step 3: Commit**

```bash
git add client/src/lib/types.ts client/src/stores/preferences.ts
git commit -m "feat(prefs): add display preferences for UI customization

- Add DisplayMode (dense/minimal/discord)
- Add reaction style and idle timeout settings
- Integrate with server-synced preferences"
```

---

## Phase 2: Voice Quality Indicators

### Task 2.1: Add ConnectionMetrics to Voice Store

**Files:**
- Modify: `client/src/stores/voice.ts`
- Modify: `client/src/lib/types.ts`

**Step 1: Add ConnectionMetrics type**

Add to `client/src/lib/types.ts`:

```typescript
export interface ConnectionMetrics {
  latency: number;
  packetLoss: number;
  jitter: number;
  quality: QualityLevel;
  timestamp: number;
}

export interface ParticipantMetrics {
  [userId: string]: ConnectionMetrics;
}
```

**Step 2: Add metrics state to voice store**

Add to voice store state:

```typescript
// In voice store state
localMetrics: ConnectionMetrics | null;
participantMetrics: ParticipantMetrics;
metricsInterval: number | null;
sessionId: string | null;
```

**Step 3: Add metrics collection function**

```typescript
function calculateQuality(latency: number, packetLoss: number, jitter: number): QualityLevel {
  if (latency > 300 || packetLoss > 5 || jitter > 60) return 'poor';
  if (latency > 100 || packetLoss > 1 || jitter > 30) return 'warning';
  if (latency >= 0) return 'good';
  return 'unknown';
}

async function collectMetrics(): Promise<ConnectionMetrics | null> {
  // Implementation depends on WebRTC adapter
  // Extract from RTCPeerConnection.getStats()
}
```

**Step 4: Commit**

```bash
git add client/src/stores/voice.ts client/src/lib/types.ts
git commit -m "feat(voice): add connection metrics tracking

- Add ConnectionMetrics type with latency/loss/jitter/quality
- Add local and participant metrics to voice store
- Add quality calculation function"
```

---

### Task 2.2: Implement Metrics Extraction from WebRTC

**Files:**
- Modify: `client/src/lib/webrtc/browser.ts`

**Step 1: Add getStats extraction**

```typescript
private prevStats: { lost: number; received: number } | null = null;

async getConnectionMetrics(): Promise<ConnectionMetrics | null> {
  if (!this.peerConnection) return null;

  const stats = await this.peerConnection.getStats();
  let latency = 0, jitter = 0;
  let totalLost = 0, totalReceived = 0;

  stats.forEach(report => {
    if (report.type === 'candidate-pair' && report.state === 'succeeded') {
      latency = (report.currentRoundTripTime ?? 0) * 1000;
    }
    if (report.type === 'inbound-rtp' && report.kind === 'audio') {
      totalLost += report.packetsLost ?? 0;
      totalReceived += report.packetsReceived ?? 0;
      jitter = Math.max(jitter, (report.jitter ?? 0) * 1000);
    }
  });

  let packetLoss = 0;
  if (this.prevStats) {
    const deltaLost = totalLost - this.prevStats.lost;
    const deltaReceived = totalReceived - this.prevStats.received;
    const total = deltaLost + deltaReceived;
    if (total > 0) packetLoss = (deltaLost / total) * 100;
  }
  this.prevStats = { lost: totalLost, received: totalReceived };

  return {
    latency: Math.round(latency),
    packetLoss: Math.round(packetLoss * 100) / 100,
    jitter: Math.round(jitter),
    quality: this.calculateQuality(latency, packetLoss, jitter),
    timestamp: Date.now(),
  };
}
```

**Step 2: Commit**

```bash
git add client/src/lib/webrtc/browser.ts
git commit -m "feat(webrtc): extract connection metrics from RTCPeerConnection

- Parse getStats() for latency, packet loss, jitter
- Calculate delta packet loss between samples
- Return ConnectionMetrics object"
```

---

### Task 2.3: Add Metrics Reporting Loop

**Files:**
- Modify: `client/src/stores/voice.ts`

**Step 1: Add 3-second reporting interval**

```typescript
const METRICS_INTERVAL_MS = 3000;

function startMetricsReporting() {
  if (voiceState.metricsInterval) return;

  const sessionId = crypto.randomUUID();
  setVoiceState({ sessionId });

  const interval = setInterval(async () => {
    try {
      const metrics = await adapter.getConnectionMetrics();
      if (metrics) {
        setVoiceState({ localMetrics: metrics });
        // Send to server
        sendWebSocketMessage({
          type: 'voice_stats',
          session_id: sessionId,
          ...metrics,
        });
      }
    } catch (err) {
      console.warn('Failed to collect metrics:', err);
    }
  }, METRICS_INTERVAL_MS);

  setVoiceState({ metricsInterval: interval });
}

function stopMetricsReporting() {
  if (voiceState.metricsInterval) {
    clearInterval(voiceState.metricsInterval);
    setVoiceState({ metricsInterval: null, sessionId: null });
  }
}
```

**Step 2: Call on connect/disconnect**

Hook into existing voice connect/disconnect handlers.

**Step 3: Commit**

```bash
git add client/src/stores/voice.ts
git commit -m "feat(voice): add metrics reporting loop

- Report metrics every 3 seconds while connected
- Generate session ID per voice connection
- Send voice_stats WebSocket messages"
```

---

### Task 2.4: Server-Side Metrics Broadcast

**Files:**
- Modify: `server/src/ws/mod.rs` or `server/src/voice/ws_handler.rs`
- Modify: `server/src/voice/messages.rs`

**Step 1: Add VoiceStats message type**

```rust
#[derive(Debug, Deserialize)]
pub struct VoiceStatsMessage {
    pub session_id: Uuid,
    pub latency: i16,
    pub packet_loss: f32,
    pub jitter: i16,
    pub quality: u8,
    pub timestamp: i64,
}

#[derive(Debug, Serialize)]
pub struct VoiceUserStats {
    pub user_id: Uuid,
    pub latency: i16,
    pub packet_loss: f32,
    pub jitter: i16,
    pub quality: u8,
}
```

**Step 2: Handle voice_stats and broadcast**

```rust
async fn handle_voice_stats(
    user_id: Uuid,
    channel_id: Uuid,
    stats: VoiceStatsMessage,
    sfu: &Sfu,
    pool: &PgPool,
) -> Result<()> {
    // Validate ranges
    if stats.latency < 0 || stats.latency > 10000 { return Ok(()); }
    if stats.packet_loss < 0.0 || stats.packet_loss > 100.0 { return Ok(()); }

    // Broadcast to room
    let broadcast = ServerEvent::VoiceUserStats(VoiceUserStats {
        user_id,
        latency: stats.latency,
        packet_loss: stats.packet_loss,
        jitter: stats.jitter,
        quality: stats.quality,
    });
    sfu.broadcast_to_room(channel_id, &broadcast).await?;

    // Store in database (fire and forget)
    tokio::spawn(store_metrics(pool.clone(), user_id, channel_id, stats));

    Ok(())
}
```

**Step 3: Commit**

```bash
git add server/src/voice/messages.rs server/src/ws/mod.rs
git commit -m "feat(server): handle voice_stats and broadcast to room

- Add VoiceStatsMessage and VoiceUserStats types
- Validate metric ranges
- Broadcast to all room participants
- Store in connection_metrics table"
```

---

### Task 2.5: Voice Quality UI Components

**Files:**
- Create: `client/src/components/voice/QualityIndicator.tsx`
- Create: `client/src/components/voice/QualityTooltip.tsx`
- Modify: `client/src/components/voice/VoiceIsland.tsx`
- Modify: `client/src/components/voice/VoiceParticipants.tsx`

**Step 1: Create QualityIndicator**

```typescript
// client/src/components/voice/QualityIndicator.tsx
import { Component, Show } from "solid-js";
import StatusIndicator from "@/components/ui/StatusIndicator";
import type { ConnectionMetrics } from "@/lib/types";
import { preferencesState } from "@/stores/preferences";

interface QualityIndicatorProps {
  metrics: ConnectionMetrics | null;
}

const QualityIndicator: Component<QualityIndicatorProps> = (props) => {
  const showNumbers = () => preferencesState.display?.showLatencyNumbers ?? true;

  return (
    <Show when={props.metrics} fallback={<StatusIndicator quality="unknown" size="sm" />}>
      {(m) => (
        <StatusIndicator
          quality={m().quality}
          size="sm"
          text={showNumbers() ? `${m().latency}ms` : undefined}
        />
      )}
    </Show>
  );
};

export default QualityIndicator;
```

**Step 2: Create QualityTooltip**

```typescript
// client/src/components/voice/QualityTooltip.tsx
import { Component, Show } from "solid-js";
import StatusIndicator from "@/components/ui/StatusIndicator";
import type { ConnectionMetrics, QualityLevel } from "@/lib/types";

interface QualityTooltipProps {
  metrics: ConnectionMetrics;
}

function getMetricQuality(value: number, thresholds: [number, number]): QualityLevel {
  if (value <= thresholds[0]) return 'good';
  if (value <= thresholds[1]) return 'warning';
  return 'poor';
}

const QualityTooltip: Component<QualityTooltipProps> = (props) => {
  const latencyQuality = () => getMetricQuality(props.metrics.latency, [100, 300]);
  const lossQuality = () => getMetricQuality(props.metrics.packetLoss, [1, 5]);
  const jitterQuality = () => getMetricQuality(props.metrics.jitter, [30, 60]);

  const worstMetric = () => {
    if (props.metrics.quality === 'poor') {
      if (props.metrics.latency > 300) return 'latency';
      if (props.metrics.packetLoss > 5) return 'packetLoss';
      return 'jitter';
    }
    if (props.metrics.quality === 'warning') {
      if (props.metrics.latency > 100) return 'latency';
      if (props.metrics.packetLoss > 1) return 'packetLoss';
      return 'jitter';
    }
    return null;
  };

  return (
    <div class="bg-background-secondary rounded-lg p-3 shadow-lg min-w-48">
      <div class="text-sm font-medium text-text-primary mb-2">Connection Quality</div>
      <div class="space-y-1.5">
        <div class="flex justify-between items-center text-xs">
          <span class="text-text-secondary">Latency</span>
          <span class="flex items-center gap-1.5">
            <span class="text-text-primary">{props.metrics.latency}ms</span>
            <StatusIndicator quality={latencyQuality()} size="xs" />
            <Show when={worstMetric() === 'latency'}>
              <span class="text-warning text-[10px]">worst</span>
            </Show>
          </span>
        </div>
        <div class="flex justify-between items-center text-xs">
          <span class="text-text-secondary">Packet Loss</span>
          <span class="flex items-center gap-1.5">
            <span class="text-text-primary">{props.metrics.packetLoss.toFixed(1)}%</span>
            <StatusIndicator quality={lossQuality()} size="xs" />
            <Show when={worstMetric() === 'packetLoss'}>
              <span class="text-warning text-[10px]">worst</span>
            </Show>
          </span>
        </div>
        <div class="flex justify-between items-center text-xs">
          <span class="text-text-secondary">Jitter</span>
          <span class="flex items-center gap-1.5">
            <span class="text-text-primary">{props.metrics.jitter}ms</span>
            <StatusIndicator quality={jitterQuality()} size="xs" />
            <Show when={worstMetric() === 'jitter'}>
              <span class="text-warning text-[10px]">worst</span>
            </Show>
          </span>
        </div>
      </div>
      <div class="mt-2 pt-2 border-t border-white/10 text-xs">
        <span class="text-text-secondary">Overall: </span>
        <span class="text-text-primary capitalize">{props.metrics.quality}</span>
      </div>
    </div>
  );
};

export default QualityTooltip;
```

**Step 3: Add to VoiceIsland and VoiceParticipants**

Integrate QualityIndicator into existing components.

**Step 4: Commit**

```bash
git add client/src/components/voice/QualityIndicator.tsx client/src/components/voice/QualityTooltip.tsx
git add client/src/components/voice/VoiceIsland.tsx client/src/components/voice/VoiceParticipants.tsx
git commit -m "feat(voice): add quality indicator UI components

- QualityIndicator with shape + optional latency text
- QualityTooltip with per-metric breakdown
- Integrated into VoiceIsland and participant list"
```

---

## Phase 3: User Presence & Status

### Task 3.1: Extend User Status Types

**Files:**
- Modify: `client/src/lib/types.ts`
- Modify: `server/src/db/models.rs` (or equivalent)

**Step 1: Update UserStatus type**

Change in `client/src/lib/types.ts`:

```typescript
// Replace existing UserStatus
export type UserStatus = 'online' | 'idle' | 'dnd' | 'invisible' | 'offline';

// Update Activity types
export type ActivityType = 'playing' | 'streaming' | 'listening' | 'watching' | 'custom';

export interface CustomStatus {
  text: string;
  emoji?: string;
  expiresAt?: string;
}

export interface Activity {
  type: ActivityType;
  name: string;
  startedAt: string;
  details?: string;
}

export interface UserPresence {
  status: UserStatus;
  customStatus?: CustomStatus | null;
  activity?: Activity | null;
  lastSeen?: string;
}
```

**Step 2: Commit**

```bash
git add client/src/lib/types.ts
git commit -m "feat(presence): extend status types

- Add dnd, invisible status options
- Add streaming activity type
- Add CustomStatus with text, emoji, expiry"
```

---

### Task 3.2: Add Idle Detection

**Files:**
- Create: `client/src/lib/idleDetector.ts`
- Modify: `client/src/stores/presence.ts`

**Step 1: Create idle detector**

```typescript
// client/src/lib/idleDetector.ts

type IdleCallback = (isIdle: boolean) => void;

let idleTimeout: number | null = null;
let isCurrentlyIdle = false;
let callback: IdleCallback | null = null;
let timeoutMs = 5 * 60 * 1000; // 5 minutes default

function resetIdleTimer() {
  if (idleTimeout) clearTimeout(idleTimeout);

  if (isCurrentlyIdle) {
    isCurrentlyIdle = false;
    callback?.(false);
  }

  idleTimeout = window.setTimeout(() => {
    isCurrentlyIdle = true;
    callback?.(true);
  }, timeoutMs);
}

const events = ['mousedown', 'mousemove', 'keydown', 'scroll', 'touchstart'];

export function startIdleDetection(onIdleChange: IdleCallback, timeoutMinutes = 5) {
  callback = onIdleChange;
  timeoutMs = timeoutMinutes * 60 * 1000;

  events.forEach(event => {
    document.addEventListener(event, resetIdleTimer, { passive: true });
  });

  resetIdleTimer();
}

export function stopIdleDetection() {
  events.forEach(event => {
    document.removeEventListener(event, resetIdleTimer);
  });

  if (idleTimeout) {
    clearTimeout(idleTimeout);
    idleTimeout = null;
  }

  callback = null;
}

export function setIdleTimeout(minutes: number) {
  timeoutMs = minutes * 60 * 1000;
  resetIdleTimer();
}
```

**Step 2: Integrate into presence store**

```typescript
// In presence store
import { startIdleDetection, stopIdleDetection, setIdleTimeout } from '@/lib/idleDetector';

let previousStatus: UserStatus = 'online';

export function initIdleDetection() {
  startIdleDetection((isIdle) => {
    if (isIdle && presenceState.users[currentUserId]?.status === 'online') {
      previousStatus = 'online';
      setMyStatus('idle');
    } else if (!isIdle && presenceState.users[currentUserId]?.status === 'idle') {
      setMyStatus(previousStatus);
    }
  });
}
```

**Step 3: Commit**

```bash
git add client/src/lib/idleDetector.ts client/src/stores/presence.ts
git commit -m "feat(presence): add idle detection

- Detect inactivity after configurable timeout
- Auto-set idle status, restore on activity
- Track mouse, keyboard, scroll, touch events"
```

---

### Task 3.3: Status Picker Component

**Files:**
- Modify: `client/src/components/ui/StatusPicker.tsx`

**Step 1: Update StatusPicker with new statuses**

```typescript
// client/src/components/ui/StatusPicker.tsx
import { Component, createSignal, Show } from "solid-js";
import StatusIndicator from "./StatusIndicator";
import type { UserStatus } from "@/lib/types";

interface StatusPickerProps {
  currentStatus: UserStatus;
  onStatusChange: (status: UserStatus) => void;
  onCustomStatusClick?: () => void;
}

const statuses: { value: UserStatus; label: string }[] = [
  { value: 'online', label: 'Online' },
  { value: 'idle', label: 'Idle' },
  { value: 'dnd', label: 'Do Not Disturb' },
  { value: 'invisible', label: 'Invisible' },
];

const StatusPicker: Component<StatusPickerProps> = (props) => {
  return (
    <div class="bg-background-secondary rounded-lg p-2 min-w-48">
      <div class="text-xs text-text-secondary px-2 py-1 mb-1">Set Status</div>
      {statuses.map(({ value, label }) => (
        <button
          class={`w-full flex items-center gap-2 px-2 py-1.5 rounded text-sm text-left transition-colors ${
            props.currentStatus === value
              ? 'bg-accent-primary/20 text-accent-primary'
              : 'text-text-primary hover:bg-white/5'
          }`}
          onClick={() => props.onStatusChange(value)}
        >
          <StatusIndicator status={value} size="sm" />
          <span>{label}</span>
        </button>
      ))}
      <Show when={props.onCustomStatusClick}>
        <div class="border-t border-white/10 mt-2 pt-2">
          <button
            class="w-full flex items-center gap-2 px-2 py-1.5 rounded text-sm text-text-secondary hover:bg-white/5"
            onClick={props.onCustomStatusClick}
          >
            <span>💬</span>
            <span>Set Custom Status...</span>
          </button>
        </div>
      </Show>
    </div>
  );
};

export default StatusPicker;
```

**Step 2: Commit**

```bash
git add client/src/components/ui/StatusPicker.tsx
git commit -m "feat(presence): update StatusPicker with all statuses

- Add online, idle, dnd, invisible options
- Show accessibility shapes for each status
- Add custom status button"
```

---

### Task 3.4: Custom Status Modal

**Files:**
- Create: `client/src/components/ui/CustomStatusModal.tsx`

**Step 1: Create modal component**

```typescript
// client/src/components/ui/CustomStatusModal.tsx
import { Component, createSignal } from "solid-js";
import type { CustomStatus } from "@/lib/types";

interface CustomStatusModalProps {
  currentStatus?: CustomStatus | null;
  onSave: (status: CustomStatus | null) => void;
  onClose: () => void;
}

const expiryOptions = [
  { value: null, label: "Don't clear" },
  { value: 30, label: '30 minutes' },
  { value: 60, label: '1 hour' },
  { value: 240, label: '4 hours' },
  { value: 1440, label: '1 day' },
];

const CustomStatusModal: Component<CustomStatusModalProps> = (props) => {
  const [text, setText] = createSignal(props.currentStatus?.text ?? '');
  const [emoji, setEmoji] = createSignal(props.currentStatus?.emoji ?? '');
  const [expiryMinutes, setExpiryMinutes] = createSignal<number | null>(null);

  const handleSave = () => {
    if (!text().trim()) {
      props.onSave(null);
    } else {
      const expiresAt = expiryMinutes()
        ? new Date(Date.now() + expiryMinutes()! * 60 * 1000).toISOString()
        : undefined;
      props.onSave({ text: text().trim(), emoji: emoji() || undefined, expiresAt });
    }
    props.onClose();
  };

  const handleClear = () => {
    props.onSave(null);
    props.onClose();
  };

  return (
    <div class="fixed inset-0 bg-black/50 flex items-center justify-center z-50" onClick={props.onClose}>
      <div class="bg-background-primary rounded-lg p-4 w-96" onClick={(e) => e.stopPropagation()}>
        <h3 class="text-lg font-semibold text-text-primary mb-4">Set Custom Status</h3>

        <div class="flex gap-2 mb-4">
          <input
            type="text"
            placeholder="😀"
            value={emoji()}
            onInput={(e) => setEmoji(e.currentTarget.value)}
            class="w-12 px-2 py-2 bg-background-secondary rounded text-center"
            maxLength={2}
          />
          <input
            type="text"
            placeholder="What's happening?"
            value={text()}
            onInput={(e) => setText(e.currentTarget.value)}
            class="flex-1 px-3 py-2 bg-background-secondary rounded text-text-primary"
            maxLength={128}
          />
        </div>

        <div class="mb-4">
          <label class="text-sm text-text-secondary mb-1 block">Clear after</label>
          <select
            value={expiryMinutes() ?? ''}
            onChange={(e) => setExpiryMinutes(e.currentTarget.value ? Number(e.currentTarget.value) : null)}
            class="w-full px-3 py-2 bg-background-secondary rounded text-text-primary"
          >
            {expiryOptions.map(({ value, label }) => (
              <option value={value ?? ''}>{label}</option>
            ))}
          </select>
        </div>

        <div class="flex justify-between">
          <button
            onClick={handleClear}
            class="px-4 py-2 text-text-secondary hover:text-text-primary transition-colors"
          >
            Clear Status
          </button>
          <div class="flex gap-2">
            <button
              onClick={props.onClose}
              class="px-4 py-2 bg-background-secondary rounded hover:bg-white/10 transition-colors"
            >
              Cancel
            </button>
            <button
              onClick={handleSave}
              class="px-4 py-2 bg-accent-primary rounded text-white hover:bg-accent-primary/90 transition-colors"
            >
              Save
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};

export default CustomStatusModal;
```

**Step 2: Commit**

```bash
git add client/src/components/ui/CustomStatusModal.tsx
git commit -m "feat(presence): add custom status modal

- Emoji picker input
- Status text with 128 char limit
- Expiry time selector
- Clear status button"
```

---

## Phase 4: Message Reactions & Emoji

### Task 4.1: Database Schema for Reactions and Custom Emojis

**Files:**
- Create: `server/migrations/YYYYMMDD_reactions_and_emojis.sql`

**Step 1: Create migration**

```sql
-- Message reactions
CREATE TABLE message_reactions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    message_id UUID NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    emoji VARCHAR(64) NOT NULL,  -- Unicode emoji or custom emoji ID
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(message_id, user_id, emoji)
);

CREATE INDEX idx_reactions_message ON message_reactions(message_id);
CREATE INDEX idx_reactions_user ON message_reactions(user_id);

-- Guild custom emojis
CREATE TABLE guild_emojis (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    guild_id UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    name VARCHAR(32) NOT NULL,
    image_url TEXT NOT NULL,
    animated BOOLEAN NOT NULL DEFAULT FALSE,
    uploaded_by UUID NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(guild_id, name)
);

CREATE INDEX idx_emojis_guild ON guild_emojis(guild_id);

-- User emoji preferences
CREATE TABLE user_emoji_favorites (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    emoji VARCHAR(64) NOT NULL,
    position INT NOT NULL DEFAULT 0,
    PRIMARY KEY(user_id, emoji)
);

CREATE TABLE user_emoji_recents (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    emoji VARCHAR(64) NOT NULL,
    used_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY(user_id, emoji)
);

CREATE INDEX idx_recents_user_time ON user_emoji_recents(user_id, used_at DESC);
```

**Step 2: Run migration**

Run: `cd server && sqlx migrate run`

**Step 3: Commit**

```bash
git add server/migrations/*_reactions_and_emojis.sql
git commit -m "feat(db): add reactions and custom emojis schema

- message_reactions with unique constraint per user+message+emoji
- guild_emojis for custom emoji storage
- user_emoji_favorites and recents tables"
```

---

### Task 4.2: Reactions API Handlers

**Files:**
- Create: `server/src/api/reactions.rs`
- Modify: `server/src/api/mod.rs`

**Step 1: Implement reaction endpoints**

```rust
// server/src/api/reactions.rs
use axum::{extract::{Path, State}, Json};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct AddReactionRequest {
    pub emoji: String,
}

pub async fn add_reaction(
    State(state): State<AppState>,
    Path((channel_id, message_id)): Path<(Uuid, Uuid)>,
    user_id: UserId,
    Json(req): Json<AddReactionRequest>,
) -> Result<impl IntoResponse, ApiError> {
    // Validate emoji (max 64 chars, valid unicode or custom ID)
    if req.emoji.len() > 64 { return Err(ApiError::BadRequest("Emoji too long")); }

    // Check message exists and user has access
    // ...

    // Insert reaction (ignore if exists)
    sqlx::query(r#"
        INSERT INTO message_reactions (message_id, user_id, emoji)
        VALUES ($1, $2, $3)
        ON CONFLICT DO NOTHING
    "#)
    .bind(message_id)
    .bind(user_id.0)
    .bind(&req.emoji)
    .execute(&state.pool)
    .await?;

    // Broadcast to channel
    // ...

    Ok(StatusCode::CREATED)
}

pub async fn remove_reaction(
    State(state): State<AppState>,
    Path((channel_id, message_id, emoji)): Path<(Uuid, Uuid, String)>,
    user_id: UserId,
) -> Result<impl IntoResponse, ApiError> {
    sqlx::query(r#"
        DELETE FROM message_reactions
        WHERE message_id = $1 AND user_id = $2 AND emoji = $3
    "#)
    .bind(message_id)
    .bind(user_id.0)
    .bind(&emoji)
    .execute(&state.pool)
    .await?;

    // Broadcast removal
    // ...

    Ok(StatusCode::NO_CONTENT)
}
```

**Step 2: Wire up routes**

```rust
// In api/mod.rs
.route("/channels/:channel_id/messages/:message_id/reactions", post(reactions::add_reaction))
.route("/channels/:channel_id/messages/:message_id/reactions/:emoji", delete(reactions::remove_reaction))
```

**Step 3: Commit**

```bash
git add server/src/api/reactions.rs server/src/api/mod.rs
git commit -m "feat(api): add reaction endpoints

- PUT to add reaction
- DELETE to remove reaction
- Broadcast changes to channel subscribers"
```

---

### Task 4.3: Emoji Picker Component

**Files:**
- Create: `client/src/components/emoji/EmojiPicker.tsx`
- Create: `client/src/components/emoji/EmojiCategory.tsx`
- Create: `client/src/stores/emoji.ts`

**Step 1: Create emoji store**

```typescript
// client/src/stores/emoji.ts
import { createStore } from "solid-js/store";

interface EmojiState {
  recents: string[];
  favorites: string[];
  guildEmojis: Record<string, GuildEmoji[]>;
  searchResults: string[];
}

const [emojiState, setEmojiState] = createStore<EmojiState>({
  recents: [],
  favorites: [],
  guildEmojis: {},
  searchResults: [],
});

export function addToRecents(emoji: string) {
  setEmojiState('recents', (prev) => {
    const filtered = prev.filter(e => e !== emoji);
    return [emoji, ...filtered].slice(0, 20);
  });
  // Persist to server
}

export function searchEmoji(query: string): string[] {
  // Search twemoji data by name/keywords
  // Return matching emoji
}

export { emojiState };
```

**Step 2: Create EmojiPicker**

```typescript
// client/src/components/emoji/EmojiPicker.tsx
import { Component, createSignal, For, Show } from "solid-js";
import { emojiState, searchEmoji, addToRecents } from "@/stores/emoji";

interface EmojiPickerProps {
  onSelect: (emoji: string) => void;
  onClose: () => void;
  guildId?: string;
}

const EMOJI_CATEGORIES = [
  { id: 'recents', name: 'Recent', emojis: [] }, // Filled dynamically
  { id: 'favorites', name: 'Favorites', emojis: [] },
  { id: 'smileys', name: 'Smileys & Emotion', emojis: ['😀','😃','😄','😁','😆','😅','🤣','😂'] },
  // ... more categories
];

const EmojiPicker: Component<EmojiPickerProps> = (props) => {
  const [search, setSearch] = createSignal('');
  const [activeCategory, setActiveCategory] = createSignal('recents');

  const handleSelect = (emoji: string) => {
    addToRecents(emoji);
    props.onSelect(emoji);
    props.onClose();
  };

  return (
    <div class="bg-background-secondary rounded-lg shadow-xl w-80 max-h-96 overflow-hidden flex flex-col">
      {/* Search */}
      <div class="p-2 border-b border-white/10">
        <input
          type="text"
          placeholder="Search emoji..."
          value={search()}
          onInput={(e) => setSearch(e.currentTarget.value)}
          class="w-full px-3 py-1.5 bg-background-primary rounded text-sm"
        />
      </div>

      {/* Categories */}
      <div class="flex-1 overflow-y-auto p-2">
        <Show when={!search()}>
          {/* Recents */}
          <Show when={emojiState.recents.length > 0}>
            <div class="mb-3">
              <div class="text-xs text-text-secondary uppercase mb-1">Recent</div>
              <div class="flex flex-wrap gap-1">
                <For each={emojiState.recents}>
                  {(emoji) => (
                    <button
                      class="w-8 h-8 flex items-center justify-center hover:bg-white/10 rounded text-xl"
                      onClick={() => handleSelect(emoji)}
                    >
                      {emoji}
                    </button>
                  )}
                </For>
              </div>
            </div>
          </Show>

          {/* Guild emojis */}
          <Show when={props.guildId && emojiState.guildEmojis[props.guildId]?.length}>
            <div class="mb-3">
              <div class="text-xs text-text-secondary uppercase mb-1">Server Emojis</div>
              <div class="flex flex-wrap gap-1">
                <For each={emojiState.guildEmojis[props.guildId!]}>
                  {(emoji) => (
                    <button
                      class="w-8 h-8 flex items-center justify-center hover:bg-white/10 rounded"
                      onClick={() => handleSelect(`:${emoji.name}:`)}
                    >
                      <img src={emoji.image_url} alt={emoji.name} class="w-6 h-6" />
                    </button>
                  )}
                </For>
              </div>
            </div>
          </Show>

          {/* Standard categories */}
          <For each={EMOJI_CATEGORIES.slice(2)}>
            {(category) => (
              <div class="mb-3">
                <div class="text-xs text-text-secondary uppercase mb-1">{category.name}</div>
                <div class="flex flex-wrap gap-1">
                  <For each={category.emojis}>
                    {(emoji) => (
                      <button
                        class="w-8 h-8 flex items-center justify-center hover:bg-white/10 rounded text-xl"
                        onClick={() => handleSelect(emoji)}
                      >
                        {emoji}
                      </button>
                    )}
                  </For>
                </div>
              </div>
            )}
          </For>
        </Show>

        {/* Search results */}
        <Show when={search()}>
          <div class="flex flex-wrap gap-1">
            <For each={searchEmoji(search())}>
              {(emoji) => (
                <button
                  class="w-8 h-8 flex items-center justify-center hover:bg-white/10 rounded text-xl"
                  onClick={() => handleSelect(emoji)}
                >
                  {emoji}
                </button>
              )}
            </For>
          </div>
        </Show>
      </div>
    </div>
  );
};

export default EmojiPicker;
```

**Step 3: Commit**

```bash
git add client/src/components/emoji/EmojiPicker.tsx client/src/stores/emoji.ts
git commit -m "feat(emoji): add emoji picker with search and categories

- Recent emojis (last 20)
- Guild custom emojis
- Standard emoji categories
- Keyboard search"
```

---

### Task 4.4: Reaction Bar Component

**Files:**
- Create: `client/src/components/messages/ReactionBar.tsx`
- Modify: `client/src/components/messages/Message.tsx`

**Step 1: Create ReactionBar**

```typescript
// client/src/components/messages/ReactionBar.tsx
import { Component, For, Show, createSignal } from "solid-js";
import EmojiPicker from "@/components/emoji/EmojiPicker";
import type { Reaction } from "@/lib/types";

interface ReactionBarProps {
  reactions: Reaction[];
  onAddReaction: (emoji: string) => void;
  onRemoveReaction: (emoji: string) => void;
  guildId?: string;
}

const ReactionBar: Component<ReactionBarProps> = (props) => {
  const [showPicker, setShowPicker] = createSignal(false);

  const handleReactionClick = (reaction: Reaction) => {
    if (reaction.me) {
      props.onRemoveReaction(reaction.emoji);
    } else {
      props.onAddReaction(reaction.emoji);
    }
  };

  return (
    <div class="flex flex-wrap items-center gap-1 mt-1">
      <For each={props.reactions}>
        {(reaction) => (
          <button
            class={`inline-flex items-center gap-1 px-2 py-0.5 rounded text-sm transition-colors ${
              reaction.me
                ? 'bg-accent-primary/20 border border-accent-primary/50'
                : 'bg-background-secondary hover:bg-white/10'
            }`}
            onClick={() => handleReactionClick(reaction)}
            title={reaction.users.join(', ')}
          >
            <span>{reaction.emoji}</span>
            <span class="text-xs text-text-secondary">{reaction.count}</span>
          </button>
        )}
      </For>

      <div class="relative">
        <button
          class="w-6 h-6 flex items-center justify-center rounded hover:bg-white/10 text-text-secondary"
          onClick={() => setShowPicker(!showPicker())}
        >
          +
        </button>
        <Show when={showPicker()}>
          <div class="absolute bottom-full left-0 mb-2 z-50">
            <EmojiPicker
              onSelect={props.onAddReaction}
              onClose={() => setShowPicker(false)}
              guildId={props.guildId}
            />
          </div>
        </Show>
      </div>
    </div>
  );
};

export default ReactionBar;
```

**Step 2: Add to Message component**

Integrate ReactionBar at bottom of each message.

**Step 3: Commit**

```bash
git add client/src/components/messages/ReactionBar.tsx client/src/components/messages/Message.tsx
git commit -m "feat(reactions): add reaction bar to messages

- Show reactions with count
- Toggle own reaction on click
- Add reaction via emoji picker"
```

---

## Phase 5: Channel Categories

### Task 5.1: Database Schema for Categories

**Files:**
- Create: `server/migrations/YYYYMMDD_channel_categories.sql`

**Step 1: Create migration**

```sql
-- Channel categories (2-level nesting max)
CREATE TABLE channel_categories (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    guild_id UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    name VARCHAR(100) NOT NULL,
    position INT NOT NULL DEFAULT 0,
    parent_id UUID REFERENCES channel_categories(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT no_deep_nesting CHECK (
        parent_id IS NULL OR NOT EXISTS (
            SELECT 1 FROM channel_categories p WHERE p.id = parent_id AND p.parent_id IS NOT NULL
        )
    )
);

CREATE INDEX idx_categories_guild ON channel_categories(guild_id);
CREATE INDEX idx_categories_parent ON channel_categories(parent_id);

-- Add category_id to channels if not exists
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'channels' AND column_name = 'category_id'
    ) THEN
        ALTER TABLE channels ADD COLUMN category_id UUID REFERENCES channel_categories(id) ON DELETE SET NULL;
        CREATE INDEX idx_channels_category ON channels(category_id);
    END IF;
END $$;

-- User collapse state (stored per-user)
CREATE TABLE user_category_collapse (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    category_id UUID NOT NULL REFERENCES channel_categories(id) ON DELETE CASCADE,
    collapsed BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY(user_id, category_id)
);
```

**Step 2: Commit**

```bash
git add server/migrations/*_channel_categories.sql
git commit -m "feat(db): add channel categories schema

- 2-level nesting constraint
- category_id on channels
- User collapse state table"
```

---

### Task 5.2: Category API Handlers

**Files:**
- Create: `server/src/api/categories.rs`
- Modify: `server/src/api/mod.rs`

**Step 1: Implement CRUD endpoints**

```rust
// server/src/api/categories.rs

#[derive(Deserialize)]
pub struct CreateCategoryRequest {
    pub name: String,
    pub parent_id: Option<Uuid>,
}

pub async fn create_category(
    State(state): State<AppState>,
    Path(guild_id): Path<Uuid>,
    user_id: UserId,
    Json(req): Json<CreateCategoryRequest>,
) -> Result<Json<Category>, ApiError> {
    // Check MANAGE_CHANNELS permission
    // Validate parent is not a subcategory (2-level max)
    // Get next position
    // Insert category

    let category = sqlx::query_as::<_, Category>(r#"
        INSERT INTO channel_categories (guild_id, name, parent_id, position)
        VALUES ($1, $2, $3, (
            SELECT COALESCE(MAX(position) + 1, 0)
            FROM channel_categories
            WHERE guild_id = $1 AND parent_id IS NOT DISTINCT FROM $3
        ))
        RETURNING *
    "#)
    .bind(guild_id)
    .bind(&req.name)
    .bind(req.parent_id)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(category))
}

pub async fn update_category(
    // ... similar pattern
) -> Result<Json<Category>, ApiError> {
    // Update name, position, parent_id
}

pub async fn delete_category(
    // ... similar pattern
) -> Result<StatusCode, ApiError> {
    // Channels in category become uncategorized
    // Subcategories also deleted (CASCADE)
}

pub async fn reorder_categories(
    // ... batch position update
) -> Result<StatusCode, ApiError> {
    // Update multiple category positions in transaction
}
```

**Step 2: Commit**

```bash
git add server/src/api/categories.rs server/src/api/mod.rs
git commit -m "feat(api): add category CRUD endpoints

- Create with optional parent
- Update name/position
- Delete (cascades subcategories)
- Batch reorder"
```

---

### Task 5.3: Category Sidebar UI

**Files:**
- Create: `client/src/components/channels/CategoryHeader.tsx`
- Modify: `client/src/components/channels/ChannelList.tsx`

**Step 1: Create CategoryHeader**

```typescript
// client/src/components/channels/CategoryHeader.tsx
import { Component, createSignal, Show } from "solid-js";
import { ChevronDown, ChevronRight, Plus, Settings } from "lucide-solid";

interface CategoryHeaderProps {
  id: string;
  name: string;
  collapsed: boolean;
  hasUnread: boolean;
  isSubcategory: boolean;
  onToggle: () => void;
  onCreateChannel?: () => void;
  onSettings?: () => void;
}

const CategoryHeader: Component<CategoryHeaderProps> = (props) => {
  const [hovering, setHovering] = createSignal(false);

  return (
    <div
      class={`flex items-center gap-1 px-2 py-1 cursor-pointer select-none group ${
        props.isSubcategory ? 'ml-3 border-l-2 border-white/10 pl-2' : ''
      }`}
      onMouseEnter={() => setHovering(true)}
      onMouseLeave={() => setHovering(false)}
      onClick={props.onToggle}
    >
      <span class="text-text-secondary w-3">
        {props.collapsed ? <ChevronRight class="w-3 h-3" /> : <ChevronDown class="w-3 h-3" />}
      </span>

      <span class={`text-xs font-semibold tracking-wide flex-1 ${
        props.isSubcategory ? 'text-text-secondary' : 'text-text-secondary uppercase'
      }`}>
        {props.name}
      </span>

      <Show when={props.hasUnread && props.collapsed}>
        <span class="w-2 h-2 rounded-full bg-white" />
      </Show>

      <Show when={hovering()}>
        <div class="flex items-center gap-1" onClick={(e) => e.stopPropagation()}>
          <Show when={props.onCreateChannel}>
            <button
              class="p-0.5 text-text-secondary hover:text-text-primary"
              onClick={props.onCreateChannel}
              title="Create Channel"
            >
              <Plus class="w-3.5 h-3.5" />
            </button>
          </Show>
          <Show when={props.onSettings}>
            <button
              class="p-0.5 text-text-secondary hover:text-text-primary"
              onClick={props.onSettings}
              title="Category Settings"
            >
              <Settings class="w-3.5 h-3.5" />
            </button>
          </Show>
        </div>
      </Show>
    </div>
  );
};

export default CategoryHeader;
```

**Step 2: Update ChannelList to use categories**

Group channels by category, render CategoryHeader, support collapse.

**Step 3: Commit**

```bash
git add client/src/components/channels/CategoryHeader.tsx client/src/components/channels/ChannelList.tsx
git commit -m "feat(channels): add collapsible category headers

- CategoryHeader with expand/collapse
- Subcategory indentation
- Unread indicator when collapsed
- Hover actions (create channel, settings)"
```

---

### Task 5.4: Category Drag-and-Drop

**Files:**
- Modify: `client/src/components/channels/ChannelList.tsx`
- Create: `client/src/components/channels/ChannelDragContext.tsx`

**Step 1: Add drag-drop context**

```typescript
// client/src/components/channels/ChannelDragContext.tsx
import { createContext, useContext, ParentComponent } from "solid-js";
import { createStore } from "solid-js/store";

interface DragState {
  draggingId: string | null;
  draggingType: 'channel' | 'category' | null;
  dropTargetId: string | null;
  dropPosition: 'before' | 'after' | 'inside' | null;
}

const [dragState, setDragState] = createStore<DragState>({
  draggingId: null,
  draggingType: null,
  dropTargetId: null,
  dropPosition: null,
});

export function startDrag(id: string, type: 'channel' | 'category') {
  setDragState({ draggingId: id, draggingType: type });
}

export function setDropTarget(id: string | null, position: 'before' | 'after' | 'inside' | null) {
  setDragState({ dropTargetId: id, dropPosition: position });
}

export function endDrag() {
  setDragState({ draggingId: null, draggingType: null, dropTargetId: null, dropPosition: null });
}

export { dragState };
```

**Step 2: Implement drag handlers in ChannelList**

Add draggable attributes, handle dragStart/dragOver/drop events.

**Step 3: Commit**

```bash
git add client/src/components/channels/ChannelDragContext.tsx client/src/components/channels/ChannelList.tsx
git commit -m "feat(channels): add drag-drop reordering for categories

- Drag channels between categories
- Drag categories to reorder
- Visual drop indicators
- Persist order to server"
```

---

## Final Tasks

### Task 6.1: Integration Testing

**Files:**
- Create: `client/src/stores/__tests__/emoji.test.ts`
- Create: `client/src/stores/__tests__/voice-metrics.test.ts`

**Step 1: Write tests for emoji store**

```typescript
describe('emoji store', () => {
  it('adds emoji to recents', () => {
    addToRecents('😀');
    expect(emojiState.recents[0]).toBe('😀');
  });

  it('limits recents to 20', () => {
    for (let i = 0; i < 25; i++) {
      addToRecents(String(i));
    }
    expect(emojiState.recents.length).toBe(20);
  });
});
```

**Step 2: Write tests for voice metrics**

```typescript
describe('voice metrics', () => {
  it('calculates quality correctly', () => {
    expect(calculateQuality(50, 0.5, 20)).toBe('good');
    expect(calculateQuality(150, 2, 40)).toBe('warning');
    expect(calculateQuality(400, 8, 100)).toBe('poor');
  });
});
```

**Step 3: Run tests**

Run: `cd client && bun run test:run`
Expected: All tests pass

**Step 4: Commit**

```bash
git add client/src/stores/__tests__/
git commit -m "test: add unit tests for emoji and voice metrics"
```

---

### Task 6.2: Update CHANGELOG

**Files:**
- Modify: `CHANGELOG.md`

**Step 1: Add entry under [Unreleased]**

```markdown
## [Unreleased]

### Added
- Voice quality indicators with latency, packet loss, jitter display
- Accessibility shapes (circle/triangle/hexagon) for colorblind users
- User status picker with Online, Idle, DND, Invisible options
- Custom status with emoji and auto-expiry
- Idle detection with configurable timeout
- Message reactions with emoji picker
- Guild custom emoji upload and management
- Emoji search, recents, and favorites
- Channel categories with 2-level nesting
- Collapsible category headers with unread indicators
- Drag-and-drop reordering for channels and categories
- Display preferences (dense/minimal/discord modes)
```

**Step 2: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs: update CHANGELOG with unified UI features"
```

---

## Verification Checklist

After completing all tasks:

1. **Voice Quality**
   - [ ] Quality indicator shows in VoiceIsland
   - [ ] Participant list shows per-user quality
   - [ ] Tooltip shows breakdown with worst metric
   - [ ] Shapes change based on quality level

2. **User Presence**
   - [ ] Can set status (online/idle/dnd/invisible)
   - [ ] Custom status with emoji works
   - [ ] Auto-idle after inactivity
   - [ ] DND suppresses notifications

3. **Reactions & Emoji**
   - [ ] Can add/remove reactions
   - [ ] Emoji picker with search works
   - [ ] Guild custom emojis display
   - [ ] Recent emojis tracked

4. **Channel Categories**
   - [ ] Categories collapse/expand
   - [ ] Subcategories indented
   - [ ] Drag-drop reorder works
   - [ ] Unread indicator when collapsed

5. **Shared System**
   - [ ] StatusIndicator uses correct shapes
   - [ ] Display preferences save and load
   - [ ] Animations are subtle (150ms fade)

---

**Build Verification:**

```bash
# Server
cd server && cargo check && cargo test

# Client
cd client && bun run test:run && bun run build
```
