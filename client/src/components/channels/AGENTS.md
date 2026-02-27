# Channel Components

**Parent:** [../AGENTS.md](../AGENTS.md)

## Purpose

Guild channel browser and management. Displays text/voice channel lists, handles channel creation, and integrates voice channel participant views.

## Key Files

### ChannelList.tsx

Main channel browser with collapsible text and voice sections.

**Features:**

- Separate sections for Text Channels and Voice Channels
- Create channel buttons (visible on section hover)
- Microphone test button (voice section)
- Voice channel participant lists
- Empty and error states

**Channel Selection:**

- Text channels → `selectChannel(id)` updates selected state
- Voice channels → `joinVoice(id)` or `leaveVoice()` toggle

**Modal Triggers:**

- Create Text Channel → Opens CreateChannelModal with type="text"
- Create Voice Channel → Opens CreateChannelModal with type="voice"
- Test Microphone → Opens MicrophoneTest modal

**Usage:**

```tsx
import ChannelList from "@/components/channels/ChannelList";

// Inside Sidebar.tsx (guild context)
<ChannelList />;
```

### ChannelItem.tsx

Individual channel row display (not shown in read files, but imported).

**Expected Props:**

- `channel` - Channel object (id, name, type)
- `isSelected` - Highlight state for text channels
- `onClick` - Click handler

### CreateChannelModal.tsx

Modal for creating new channels.

**Expected Props:**

- `guildId` - Guild to create channel in
- `initialType` - "text" or "voice"
- `onClose` - Close callback
- `onCreated` - Success callback with channelId

**Behavior:**

- Auto-selects text channels after creation
- Voice channels auto-join after creation (likely)

## State Management

### From Stores

- `channelsState.channels` - All channels for active guild
- `channelsState.selectedChannelId` - Active text channel
- `channelsState.isLoading` - Loading state
- `channelsState.error` - Error message
- `textChannels()` - Filtered text channels
- `voiceChannels()` - Filtered voice channels
- `guildsState.activeGuildId` - Guild context for creation

### From Voice Store

- `isInChannel(id)` - Check if user in voice channel
- `joinVoice(id)` - Join voice channel
- `leaveVoice()` - Leave current voice channel

## Integration Points

### Components

- `VoiceParticipants` - Shows users in voice channel (from `../voice/`)
- `MicrophoneTest` - Mic testing modal (from `../voice/`)
- `CreateChannelModal` - Channel creation form

### Stores

- `@/stores/channels` - Channel data and selection
- `@/stores/guilds` - Guild context
- `@/stores/voice` - Voice channel state

## Styling

**Section Headers:**

- Hover reveals create buttons
- Uppercase text with `tracking-wider`
- Chevron icon (currently non-collapsible)

**Channel Items:**

- Selected text channels highlighted
- Voice channels show join state via hover/color
- 0.5 spacing between items

**Empty/Error States:**

- Center-aligned with `text-text-secondary`
- Error uses `--color-error-text`

## UX Patterns

### Voice Channel Interaction

- Click to join if not in channel
- Click again to leave (toggle behavior)
- Shows participants below channel when users present

### Microphone Test Access

- Available in voice section header (hover to reveal)
- Allows pre-testing before joining channel
- Non-blocking modal

### Auto-Selection

- Text channels auto-select after creation
- Improves UX by immediately showing new channel

## Permissions

Expected permission checks (not yet implemented):

- Channel create requires guild permissions
- Channel visibility based on user roles
- Voice channel join restrictions

## Future Enhancements

- Collapsible channel sections (chevron currently static)
- Channel categories/folders
- Channel reordering (drag-and-drop)
- Channel settings/edit
- Channel permissions management

## Related Documentation

- Channel types: `PROJECT_SPEC.md` § Channels
- Voice architecture: `ARCHITECTURE.md` § Voice Service
- WebRTC setup: `STANDARDS.md` § WebRTC
