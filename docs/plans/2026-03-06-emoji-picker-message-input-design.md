# Emoji Picker in Message Input — Design

**Date:** 2026-03-06
**Status:** Approved
**Scope:** Wire existing EmojiPicker into MessageInput for emoji insertion at cursor

## Context

The `EmojiPicker` and `PositionedEmojiPicker` components are fully built (search, recents, guild custom emojis, categories) and used for message reactions in `MessageActions`. The `MessageInput` has no emoji button — users can only insert emojis via `:query` autocomplete. This adds a smiley button to the composer.

## Design

### Approach: Button inside MessageInput (single file change)

All changes in `MessageInput.tsx`. No new components or files.

### Layout

Current: `[Attach] [textarea] [char counter | send]`
New: `[Attach] [textarea] [Smile button] [char counter | send]`

The `Smile` icon button sits between the textarea and the send area, matching Discord/Slack placement.

### Components Used

- `PositionedEmojiPicker` — existing, handles floating-ui positioning, click-outside, Escape, portal rendering
- `Smile` icon from `lucide-solid`

### Behavior

1. **Toggle** — clicking the smiley button toggles `showEmojiPicker` signal
2. **Positioning** — `PositionedEmojiPicker` anchors to the smiley button ref, positions above with floating-ui auto-flip
3. **Selection** — on emoji select, insert at saved cursor position, close picker (single-pick)
4. **Cursor tracking** — store `lastCursorPos` signal, updated on input/click/keydown. When picker opens, this determines insertion point. Falls back to end-of-text.
5. **After insert** — refocus textarea, set cursor after emoji, trigger resize and draft save

### Insertion Logic

Same substring pattern as existing `handleAutocompleteSelect`:
```typescript
const pos = lastCursorPos() ?? currentContent.length;
const before = currentContent.substring(0, pos);
const after = currentContent.substring(pos);
const newContent = before + emoji + after;
const newCursorPos = pos + emoji.length;
```

## Files Changed

| File | Change |
|------|--------|
| `client/src/components/messages/MessageInput.tsx` | Add smiley button, PositionedEmojiPicker, cursor tracking, insertion handler |
