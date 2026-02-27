# Voice Components

**Parent:** [../AGENTS.md](../AGENTS.md)

## Purpose

Voice channel UI components. Voice controls, participant lists, audio device settings, and microphone testing.

## Key Files

### VoiceControls.tsx

Mute, deafen, and settings buttons for voice channels.

**Controls:**

- **Mute Button** - Toggle microphone on/off
- **Deafen Button** - Toggle audio output on/off (auto-mutes mic)
- **Settings Button** - Opens MicrophoneTest modal

**Visual States:**

- **Muted:** Red background (`bg-danger/20 text-danger`)
- **Deafened:** Red background, also mutes mic
- **Normal:** Gray background
- **Disabled:** When not connected to voice

**Behavior:**

- Deafen automatically mutes (can't speak while deafened)
- Disabled when `voiceState.state !== "connected"`
- Settings opens mic test for device selection

**Usage:**

```tsx
import VoiceControls from "@/components/voice/VoiceControls";

// In UserPanel.tsx or VoiceIsland.tsx
<VoiceControls />;
```

**State:**

- `voiceState.muted` - Mic muted
- `voiceState.deafened` - Audio deafened
- `voiceState.state` - Connection state

### VoicePanel.tsx

Expected full voice channel panel (not shown in files read).

**Expected Structure:**

- Channel name header
- VoiceParticipants list
- VoiceControls at bottom
- Leave channel button

### VoiceParticipants.tsx

List of users in a voice channel.

**Expected Display:**

- User avatars
- Speaking indicator (green ring when talking)
- Mute/deafen icons (for other users)
- Self-mute/self-deafen indicators
- Volume slider per user (hover)

**Props:**

- `channelId: string` - Voice channel to show participants for

**Features:**

- Auto-update when users join/leave
- Visual feedback for speaking (voice activity)
- Right-click menu (user profile, kick, etc.)

### AudioDeviceSettings.tsx

Audio input/output device selection.

**Expected UI:**

- Input device dropdown (microphones)
- Output device dropdown (speakers/headphones)
- Test button for each device
- Volume meters
- Echo test (speak and hear yourself with delay)

**Device Enumeration:**

- Uses Web Audio API or Tauri command
- Lists available devices
- Shows default device
- Remembers user selection

### MicrophoneTest.tsx

Microphone testing and device selection modal.

**Features:**

- Input device dropdown
- Real-time volume meter
- Input sensitivity slider
- "Let's Check" test button
- Noise gate threshold visualization

**Workflow:**

1. User speaks into mic
2. Volume meter shows input level
3. Adjust sensitivity if too quiet/loud
4. Confirm device works
5. Save settings

**Props:**

- `onClose: () => void` - Close callback

**Usage:**

```tsx
<Show when={showMicTest()}>
  <MicrophoneTest onClose={() => setShowMicTest(false)} />
</Show>
```

## Voice State Machine

### Connection States

- `idle` - Not in voice channel
- `connecting` - Joining voice channel
- `connected` - Active in voice channel
- `reconnecting` - Connection lost, retrying
- `disconnected` - Left voice channel

### Audio States

- `muted` - Mic off (local)
- `deafened` - Audio output off (local)
- `suppressed` - Server-muted (remote)
- `speaking` - Voice activity detected

## Integration Points

### Stores

- `@/stores/voice` - Voice state, mute/deafen actions
  - `voiceState.channelId` - Current voice channel
  - `voiceState.muted` - Mic state
  - `voiceState.deafened` - Audio state
  - `voiceState.participants` - Users in channel
  - `toggleMute()`, `toggleDeafen()` - Actions

### Tauri Backend

- `getAudioDevices()` - List input/output devices
- `setAudioDevice(deviceId, type)` - Change device
- `getAudioLevel()` - Microphone volume level
- `startVoice(channelId)` - Join voice channel
- `stopVoice()` - Leave voice channel

### WebRTC (via Rust Core)

- Voice data transmitted via WebRTC
- Server acts as SFU (Selective Forwarding Unit)
- DTLS-SRTP for encryption (MVP)
- Future: MLS for E2EE "Paranoid Mode"

## Voice Processing Pipeline

```
Microphone Input
  → Web Audio API / cpal (Rust)
  → Noise Suppression (RNNoise future)
  → Voice Activity Detection (VAD)
  → Opus Encoding (48kHz, 20ms frames)
  → WebRTC Transport (DTLS-SRTP)
  → Server SFU
  → Other Clients
  → Opus Decoding
  → Audio Output (speakers/headphones)
```

## Speaking Indicators

### Local User

- Green ring around avatar when speaking
- Based on local VAD (Voice Activity Detection)
- Updates in real-time

### Remote Users

- Server sends speaking events via WebSocket
- `voice.speaking.start` - User started speaking
- `voice.speaking.stop` - User stopped speaking

## Audio Quality

### Opus Settings

- Sample rate: 48kHz
- Frame size: 20ms (960 samples)
- Bitrate: 64kbps (voice), 128kbps (music mode future)
- Channels: Mono (stereo for music mode future)

### Latency Targets

- End-to-end: <50ms (goal: 10-20ms)
- Codec latency: ~5ms (Opus)
- Network latency: 10-30ms (depends on connection)
- Jitter buffer: 20-50ms

## Device Management

### Auto-Selection

- Use default system device on first launch
- Remember user selection in localStorage
- Auto-switch when default device changes (optional)

### Hot-Plugging

- Detect when devices added/removed
- Prompt user if active device disconnected
- Fallback to system default

## Accessibility

### Voice Controls

- Keyboard shortcuts (Ctrl+Shift+M for mute)
- Screen reader announcements for state changes
- Visual indicators (not just audio)

### Alternative Communication

- Always show text chat alongside voice
- Typing indicator for hearing-impaired users
- Captions/transcription (future)

## Performance Considerations

### Voice Island

- Minimal re-renders (memoized participant list)
- Efficient speaking indicator updates
- Lazy load participant avatars

### Audio Processing

- Runs in Rust core (separate thread)
- No main thread blocking
- Low CPU usage (<1% idle, <5% active)

## Future Enhancements

### Voice Features

- Push-to-talk mode
- Voice recording/playback test
- Noise suppression (RNNoise)
- Echo cancellation
- Automatic gain control (AGC)
- Music mode (stereo, higher bitrate)

### UI Enhancements

- Waveform visualization
- Volume normalization per user
- Spatial audio (future)
- User nicknames in voice
- Screen share preview

### Advanced Settings

- Codec selection (Opus options)
- Packet loss concealment
- Forward error correction
- Jitter buffer tuning

## Related Documentation

- Voice architecture: `ARCHITECTURE.md` § Voice Service
- WebRTC setup: `STANDARDS.md` § WebRTC
- Opus codec: `STANDARDS.md` § Audio Codecs
- DTLS-SRTP: `STANDARDS.md` § Security
- MLS E2EE: `docs/mls-paranoid-mode.md` (future)
