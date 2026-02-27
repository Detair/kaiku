# Call Components

**Parent:** [../AGENTS.md](../AGENTS.md)

## Purpose

Direct message call UI components. Displays call state banners with dynamic controls for incoming, outgoing, and active calls.

## Key Files

### CallBanner.tsx

Call status banner shown at top of DM conversations.

**Call States:**

- `incoming_ringing` - Show Accept/Decline buttons with caller name
- `outgoing_ringing` - Show Cancel button with "Calling..." text
- `connecting` - Show connecting spinner
- `connected` - Show duration, participant count, Leave button
- `reconnecting` - Show retry countdown
- `ended` - Show end reason with duration

**Features:**

- Call duration timer (updates every second)
- Loading states during transitions
- End reason text mapping (cancelled, declined, no_answer, etc.)
- Participant count badge (when >0)

**Error Handling:**

- Catches 404 (call not found) and 409 (conflict) gracefully
- Resets local state on error
- Logs errors to console for debugging

**Usage:**

```tsx
import CallBanner from "@/components/call/CallBanner";

// Inside DM conversation view
<CallBanner channelId={conversation.channelId} />;
```

**Tauri Commands:**

- `joinDMCall(channelId)` - Accept/join call
- `declineDMCall(channelId)` - Decline/reject call
- `leaveDMCall(channelId)` - Leave active call

**State Management:**

- `callState.currentCall` - Current call status
- `joinCall()`, `declineCall()`, `endCall()` - Store actions

### index.ts

Re-exports CallBanner for cleaner imports.

## Call Flow

1. **Incoming Call:**
   - Initiator name displayed from call state
   - Accept → joins call, transitions to "connecting"
   - Decline → sends decline to server, removes call state

2. **Outgoing Call:**
   - Show "Calling..." with Cancel button
   - Cancel → sends leave to server, marks as "cancelled"

3. **Active Call:**
   - Duration timer starts from `call.startedAt`
   - Participant count shows number of other users
   - Leave → sends leave to server, marks as "last_left"

4. **Reconnecting:**
   - Shows countdown from call state
   - User can't interact during reconnect

5. **Ended:**
   - Shows end reason and duration (if available)
   - Auto-clears after timeout (handled by store)

## Integration Points

### Stores

- `@/stores/call` - Call state, join/decline/end actions

### Tauri Backend

- `@/lib/tauri` - Direct call commands (joinDMCall, declineDMCall, leaveDMCall)

### WebSocket Events

- Server sends call state changes via WebSocket
- Store updates trigger CallBanner re-renders

## Styling

Uses design system:

- `bg-surface-layer2` - Banner background
- `bg-green-500/20` - Accept button (incoming calls)
- `bg-red-500/20` - Decline/Leave buttons
- `bg-accent-primary/20` - Outgoing call state
- `animate-pulse` - Ringing visual feedback

## Performance

- Duration timer only runs for connected calls (cleanup via `onCleanup`)
- Loading states prevent double-clicks
- Channel ID filtering prevents wrong banner display

## Future Enhancements

Expected call features:

- Screen sharing controls
- Camera toggle (video calls)
- Call history display
- Call quality indicators
- Multi-participant DM calls (group calls)

## Related Documentation

- Call state machine: `docs/plans/call-state-machine.md` (if exists)
- WebSocket protocol: `STANDARDS.md` § WebSocket
- Voice architecture: `ARCHITECTURE.md` § Voice Service
