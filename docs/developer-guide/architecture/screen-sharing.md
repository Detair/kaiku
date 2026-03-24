# Screen Sharing Architecture

This document explains how screen sharing works in Kaiku, from the moment a user clicks "Share Screen" to the video appearing on other participants' screens.

## Overview

Kaiku uses an **SFU (Selective Forwarding Unit)** architecture. The server doesn't encode or decode video — it receives RTP packets from the publisher and forwards them to all subscribers. Each user has two WebRTC connections:

- **Publisher PC** — sends the user's own tracks (mic, screen share, webcam) to the server
- **Subscriber PC** — receives other users' tracks from the server

```mermaid
graph LR
    subgraph Alice [Alice's Browser]
        A_Pub[Publisher PC]
        A_Sub[Subscriber PC]
    end

    subgraph Server [Kaiku SFU]
        SFU[Track Router]
    end

    subgraph Bob [Bob's Browser]
        B_Pub[Publisher PC]
        B_Sub[Subscriber PC]
    end

    A_Pub -->|screen share RTP| SFU
    SFU -->|forwarded RTP| B_Sub
    B_Pub -->|mic RTP| SFU
    SFU -->|forwarded RTP| A_Sub
```

## The Complete Flow

### Step 1: User Starts Screen Share

When Alice clicks "Share Screen", the browser captures the screen using `getDisplayMedia()`:

```mermaid
sequenceDiagram
    participant Alice as Alice's Browser
    participant PubPC as Publisher PC
    participant WS as WebSocket

    Alice->>Alice: navigator.mediaDevices.getDisplayMedia()
    Alice->>PubPC: addTrack(videoTrack, stream)
    Note over PubPC: onnegotiationneeded fires
    Alice->>WS: VoiceScreenShareStart {stream_id, quality}
```

The quality setting controls resolution and framerate:

| Quality | Resolution | FPS | Bitrate |
|---------|-----------|-----|---------|
| Low | 854×480 | 15 | 500 Kbps |
| Medium | 1280×720 | 30 | 1.5 Mbps |
| High | 1920×1080 | 30 | 3 Mbps |
| Premium | 1920×1080 | 60 | 5 Mbps |

### Step 2: SDP Negotiation (Publisher)

The client creates an offer and sends it to the server. The server creates an answer. This is the **publisher** SDP exchange — only the publishing client and server participate.

```mermaid
sequenceDiagram
    participant Client as Alice's Browser
    participant Server as SFU Server

    Client->>Client: publisherPC.createOffer()
    Client->>Client: publisherPC.setLocalDescription(offer)
    Client->>Server: VoicePublisherOffer {sdp}
    Server->>Server: publisherPC.setRemoteDescription(offer)
    Server->>Server: publisherPC.createAnswer()
    Server->>Server: publisherPC.setLocalDescription(answer)
    Server->>Client: VoicePublisherAnswer {sdp}
    Client->>Client: publisherPC.setRemoteDescription(answer)
    Note over Client,Server: Publisher PC connected — RTP flows
```

### Step 3: Server Receives the Track

When the SDP exchange completes, the server's `on_track` callback fires. This is where the heavy lifting happens:

```mermaid
flowchart TD
    A[on_track fires with TrackRemote] --> B[Parse simulcast RID]
    B --> C[Pop TrackSource from pending queue]
    C --> D[Spawn RTP Forwarder task]
    D --> E[Spawn Interval PLI task - every 3s]
    E --> F{Primary layer?}
    F -->|Yes| G[Store as incoming track]
    F -->|No| H[Store as simulcast secondary]
    G --> I[For each existing subscriber peer]
    I --> J[Create subscriber TrackLocalStaticRTP]
    J --> K[Add to subscriber's outgoing tracks]
    K --> L[Renegotiate subscriber PC]
```

**Key detail: Interval PLI.** The server spawns a task that sends a Picture Loss Indication (PLI) to the publisher every 3 seconds. This forces the publisher's browser to generate keyframes, ensuring any subscriber joining later gets a fresh keyframe within 3 seconds. Without this, subscribers see a black screen until a natural keyframe arrives (which can take minutes for static screen content).

> **Why interval PLI?** webrtc-rs's `PeerConnection::write_rtcp()` doesn't reliably deliver one-shot PLI to the remote browser. The Pion SFU pattern of sending PLI on a fixed interval is the proven workaround.

### Step 4: RTP Forwarding

The RTP forwarder reads packets from the publisher's track and writes them to all subscriber tracks:

```mermaid
flowchart LR
    subgraph Publisher
        PT[Publisher TrackRemote]
    end

    subgraph "RTP Forwarder (tokio task)"
        RF[Read RTP packet]
        FW[forward_rtp]
    end

    subgraph Subscribers
        S1[Bob's TrackLocalStaticRTP]
        S2[Carol's TrackLocalStaticRTP]
    end

    PT --> RF --> FW
    FW --> S1
    FW --> S2
```

The `TrackRouter` manages subscriptions using a lock-free `DashMap`:

```
subscriptions: Map<(source_user_id, TrackSource), Vec<Subscription>>
```

Each `Subscription` contains:
- `subscriber_id` — who receives this track
- `local_track` — the `TrackLocalStaticRTP` to write RTP packets to
- `active_layer` — which simulcast layer is active (High/Medium/Low)

For non-simulcast sources (screen share via `addTrack` without RID), all packets are forwarded regardless of `active_layer`.

### Step 5: SDP Negotiation (Subscriber)

After creating subscriber tracks, the server renegotiates each subscriber's PC:

```mermaid
sequenceDiagram
    participant Server as SFU Server
    participant Bob as Bob's Browser

    Server->>Server: subscriberPC.createOffer()
    Server->>Server: subscriberPC.setLocalDescription(offer)
    Server->>Bob: VoiceSubscriberOffer {sdp}
    Bob->>Bob: subscriberPC.setRemoteDescription(offer)
    Bob->>Bob: subscriberPC.createAnswer()
    Bob->>Bob: subscriberPC.setLocalDescription(answer)
    Bob->>Server: VoiceSubscriberAnswer {sdp}
    Server->>Server: subscriberPC.setRemoteDescription(answer)
    Note over Server,Bob: Subscriber PC updated — new track flows
```

### Step 6: Video Arrives at Subscriber

When the subscriber's browser receives the new track via `ontrack`:

```mermaid
flowchart TD
    A[subscriberPC.ontrack fires] --> B[Parse stream.id to get userId + sourceType]
    B --> C{sourceType matches screen_video:*?}
    C -->|Yes| D[Extract stream_id from source]
    D --> E[Fire onScreenShareTrack event]
    E --> F[screenShareViewer store adds to availableTracks]
    F --> G[VoiceTileGrid detects new screen share]
    G --> H[Auto-focus: set as focused tile]
    H --> I[ScreenShareTile renders video element]
    I --> J[attachStream ref callback sets srcObject]
```

The stream ID format used in WebRTC track naming:
```
"{user_id}:{source_type}"
e.g. "58b286b0-...:screen_video:9e23fc36-..."
```

The client parses this to determine the source user and type.

## Codec: VP8 Only

All video uses VP8 (payload type 96). The server registers only VP8 in its `MediaEngine` — no VP9 or H.264. This guarantees that both publisher and subscriber sessions negotiate the same payload type, which is critical because the SFU forwards raw RTP packets without rewriting headers.

```
Publisher browser ←→ Server publisher PC: VP8 PT 96
Server subscriber PC ←→ Subscriber browser: VP8 PT 96
```

If different payload types were negotiated on each side, the subscriber's decoder would receive packets with an unrecognized PT and show a black screen.

## ICE and Connectivity

Each PeerConnection exchanges ICE candidates independently, tagged with a `PcType` enum (`Publisher` or `Subscriber`):

```mermaid
sequenceDiagram
    participant Client as Browser
    participant Server as SFU

    Client->>Server: VoiceIceCandidate {pc_type: Publisher, candidate}
    Server->>Client: VoiceIceCandidate {pc_type: Subscriber, candidate}
    Client->>Server: VoiceIceCandidate {pc_type: Subscriber, candidate}
```

STUN/TURN servers are fetched from the API at connection time. If TURN is unavailable, the client falls back to a public STUN server with a UI warning.

## Screen Share UI

The voice channel view uses a **tile-based layout** with two modes:

```mermaid
stateDiagram-v2
    [*] --> GridMode: No screen share active
    GridMode --> FocusMode: Screen share starts\n(auto-focus on remote share)
    FocusMode --> GridMode: Screen share stops\nor user presses Escape
    FocusMode --> FocusMode: Click different tile\n(switch focus)
    GridMode --> FocusMode: User clicks any tile

    state GridMode {
        [*] --> EqualTiles: Square-fit algorithm\nsizes tiles to fill space
    }

    state FocusMode {
        [*] --> SidebarStrip: ≤6 tiles in strip
        [*] --> BottomStrip: >6 tiles in strip
    }
```

**Grid Mode:** All participant + screen share tiles displayed equally using a square-fit algorithm (4:3 aspect ratio, max 5 columns).

**Focus Mode:** One large tile (the focused screen share) + remaining tiles in a sidebar (≤6) or bottom strip (>6).

**Pop-out:** Screen share tiles have a pop-out button that opens the stream in a separate browser window. The main tile shows "Popped out" with a "Bring back" button.

## Late Joiner Flow

When Bob joins a channel where Alice is already sharing:

```mermaid
sequenceDiagram
    participant Alice as Alice (sharing)
    participant Server as SFU
    participant Bob as Bob (joining)

    Bob->>Server: VoiceJoin
    Server->>Server: subscribe_to_existing_tracks()
    Note over Server: Creates subscriber track<br/>for Alice's screen share
    Server->>Bob: VoiceSubscriberOffer (with screen share track)
    Bob->>Server: VoiceSubscriberAnswer
    Note over Server: Subscriber PC ready
    Note over Server: Interval PLI task already running<br/>(every 3s since Alice started sharing)
    Note over Alice: Next PLI arrives within 0-3s
    Alice-->>Server: Keyframe (VP8 IDR)
    Server-->>Bob: Forwarded keyframe
    Note over Bob: Video renders!
```

The maximum wait time for a keyframe is 3 seconds, determined by the PLI interval.

## Screen Share Stop Flow

```mermaid
sequenceDiagram
    participant Alice as Alice's Browser
    participant Server as SFU
    participant Bob as Bob's Browser

    Alice->>Alice: publisherPC.removeTrack(videoSender)
    Alice->>Alice: stream.getTracks().forEach(t => t.stop())
    Alice->>Server: VoiceScreenShareStop {stream_id}
    Note over Alice: onnegotiationneeded fires → new offer

    Server->>Server: Remove from track_router
    Server->>Server: Remove from peer.incoming_tracks
    Server->>Server: Decrement Redis screen share counter

    loop For each subscriber
        Server->>Server: Remove outgoing track
        Server->>Bob: VoiceSubscriberOffer (without screen track)
    end

    Server->>Bob: ScreenShareStopped event
    Bob->>Bob: Remove track from UI
```

## Key Files

| File | Purpose |
|------|---------|
| `server/src/voice/sfu.rs` | SFU core: PeerConnection setup, on_track handler, PLI interval, renegotiation |
| `server/src/voice/track.rs` | TrackRouter: RTP forwarding, subscriptions, simulcast layer selection |
| `server/src/voice/ws_handler.rs` | WebSocket message handlers for all voice signaling |
| `server/src/voice/peer.rs` | Peer state: publisher/subscriber PCs, track storage |
| `client/src/lib/webrtc/browser.ts` | Browser WebRTC adapter: screen capture, SDP negotiation, track handling |
| `client/src/components/voice/VoiceTileGrid.tsx` | Tile layout manager: grid/focus modes, auto-focus |
| `client/src/components/voice/VoiceTile.tsx` | Individual tile: video attachment, pop-out button |
| `client/src/components/voice/screenSharePopOut.ts` | Pop-out window management |
| `shared/vc-common/src/protocol/mod.rs` | Protocol message types (ClientEvent, ServerEvent, PcType) |
