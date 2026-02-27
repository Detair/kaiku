# Message Components

**Parent:** [../AGENTS.md](../AGENTS.md)

## Purpose

Chat message display and input. Handles message rendering, grouping, virtualization, typing indicators, and message composition.

## Key Files

### MessageList.tsx

Virtualized message list with auto-scroll and new message indicators.

**Features:**

- Infinite scroll (load more on scroll to top)
- Auto-scroll on new messages (when at bottom)
- "New messages" button when scrolled up
- Message grouping (compact mode for consecutive messages)
- Loading states (initial and pagination)
- Empty state

**Scroll Behavior:**

- **At bottom (<100px from bottom):** Auto-scroll on new messages
- **Scrolled up:** Show "N new messages" button
- **Initial load:** Scroll to bottom instantly (no animation)
- **New messages:** Smooth scroll to bottom

**Message Grouping:**

- Uses `shouldGroupWithPrevious()` from utils
- Groups messages <5min apart from same author
- Compact mode hides avatar and name

**Props:**

- `channelId: string` - Channel to display messages for

**State Tracking:**

- `isAtBottom` - User scroll position
- `hasNewMessages` - New messages arrived while scrolled up
- `newMessageCount` - Count for indicator badge

**Performance:**

- Memoized message list (`createMemo`)
- Passive scroll listener
- Cleanup on component unmount

**Usage:**

```tsx
import MessageList from "@/components/messages/MessageList";

<MessageList channelId={channel.id} />;
```

### MessageItem.tsx

Individual message row renderer.

**Display Modes:**

- **Full:** Avatar, username, timestamp, content
- **Compact:** Content only (grouped with previous)

**Expected Props:**

- `message` - Message object
- `compact: boolean` - Use compact rendering

**Message Features:**

- Author avatar with status
- Timestamp (formatted)
- Message content (markdown rendered)
- Reactions (future)
- Reply thread (future)
- Attachments (images, files)

**Hover Actions:**

- Reply button
- React button
- More options (edit, delete, pin)

### MessageInput.tsx

Message composition input with typing indicators.

**Features:**

- Multi-line text input
- Send on Enter (Shift+Enter for newline)
- File attachment button
- Emoji picker button
- Character limit indicator
- Typing indicator broadcast

**Props:**

- `channelId: string` - Channel to send messages to

**Keyboard Shortcuts:**

- `Enter` - Send message (if not empty)
- `Shift+Enter` - Insert newline
- `Ctrl/Cmd+V` - Paste (including images)

**Upload Support:**

- Drag-and-drop files
- Paste images from clipboard
- File size validation (10MB limit)
- Image preview before send

### TypingIndicator.tsx

Shows "User is typing..." indicator at bottom of message list.

**Display:**

- Animated dots
- Shows up to 3 users typing
- Auto-clear after 5s without updates

**WebSocket Integration:**

- Sends typing event on keystroke (throttled to 3s)
- Receives typing events from other users

## Message Rendering

### Markdown Support

Expected markdown features:

- **Bold** (`**text**`)
- _Italic_ (`*text*`)
- `Code` (`` `code` ``)
- `Code blocks` (with syntax highlighting)
- [Links](url)
- > Quotes
- Lists (bulleted, numbered)

### User Mentions

- `@username` - Highlight and link
- `@everyone` - Special highlight (if permissions allow)

### Channel Links

- `#channel-name` - Link to channel

### Emoji

- `:emoji:` - Convert to emoji
- Custom guild emoji (future)

## State Management

### From Stores

- `messagesState.byChannel[channelId]` - Messages for channel
- `messagesState.loadingChannels[channelId]` - Loading state
- `loadInitialMessages(channelId)` - Load first page
- `loadMoreMessages(channelId)` - Pagination

### WebSocket Events

- `message.created` - New message from server
- `message.updated` - Edit
- `message.deleted` - Deletion
- `typing.start` - User started typing
- `typing.stop` - User stopped typing

## Integration Points

### Components

- `CodeBlock` (from `@/components/ui`) - Syntax-highlighted code
- `Avatar` (from `@/components/ui`) - Author avatar

### Stores

- `@/stores/messages` - Message data and actions
- `@/stores/auth` - Current user info for send
- `@/stores/channels` - Channel context

### Tauri Commands

- `sendMessage(channelId, content)` - Send message
- `uploadFile(channelId, file)` - Upload attachment
- `editMessage(messageId, content)` - Edit message
- `deleteMessage(messageId)` - Delete message

## Message Grouping Logic

From `@/lib/utils.shouldGroupWithPrevious()`:

```ts
// Group if:
// - Same author
// - Less than 5 minutes apart
// - No system message
return (
  message.author.id === prev.author.id &&
  timeDiff < 5 * 60 * 1000 &&
  !message.system &&
  !prev.system
);
```

## Performance Optimizations

### Virtualization (Future)

- Render only visible messages
- Recycle DOM nodes
- Maintain scroll position

### Current Optimizations

- Memoized message list
- Memoized compact flag computation
- Passive scroll listener
- Throttled typing events

## Empty States

### No Messages

- Welcome icon (wave emoji)
- "No messages yet" text
- "Be the first to send a message" prompt

### Loading

- Spinner at top (pagination)
- Centered spinner (initial load)
- "Loading messages..." text

## Accessibility

Expected a11y features:

- ARIA labels for input
- Keyboard navigation
- Screen reader announcements for new messages
- Focus management

## Future Enhancements

- Message reactions
- Reply threads
- Message search
- Pin messages
- Rich embeds (link previews)
- Voice message recording
- GIF/sticker picker
- Message translation
- Read receipts

## Related Documentation

- Message format: `PROJECT_SPEC.md` § Messages
- WebSocket protocol: `STANDARDS.md` § WebSocket
- E2EE implementation: `ARCHITECTURE.md` § E2EE (vodozemac)
- File upload: `docs/file-upload.md`
