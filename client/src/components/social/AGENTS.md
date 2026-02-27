# Social Components

**Parent:** [../AGENTS.md](../AGENTS.md)

## Purpose

Friends list, friend requests, and user profile management. Social graph interface for adding, accepting, and managing friend relationships.

## Key Files

### FriendsList.tsx

Main friends interface with multi-tab view.

**Tabs:**

- **Online** - Currently online friends only
- **All** - All accepted friends
- **Pending** - Incoming/outgoing friend requests
- **Blocked** - Blocked users

**Features:**

- Real-time online status
- Accept/reject pending requests
- Remove friends with confirmation
- "Add Friend" button (opens AddFriend modal)
- Tab counts update reactively

**Friend Actions:**

- **Pending tab:** Accept or Reject buttons
- **All/Online tabs:** Remove button (with confirm dialog)
- **Blocked tab:** Unblock button (future)

**Empty States:**

- "No {tab} friends" when empty
- "Loading..." during initial fetch

**Usage:**

```tsx
import { FriendsList } from "@/components/social";

// Shown in HomeView when isShowingFriends=true
<FriendsList />;
```

**Data Loading:**

- Loads on mount: `loadFriends()`, `loadPendingRequests()`, `loadBlocked()`
- Parallel fetch for faster initial load

### AddFriend.tsx

Modal for sending friend requests.

**Expected UI:**

- Username input field
- Validation (user exists, not already friends)
- Send request button
- Recent/suggested friends (future)

**Workflow:**

1. User enters username
2. Validate username exists
3. Check not already friends
4. Send friend request
5. Show success/error message
6. Close modal on success

**Props:**

- `onClose: () => void` - Close callback

### FriendItem (internal component)

Individual friend row in FriendsList.

**Display:**

- Avatar with online status indicator
- Display name (bold)
- Username (@handle)
- Status message (if set)

**Online Indicator:**

- Green dot when `is_online === true`
- Hidden for blocked users
- Positioned at bottom-right of avatar

**Actions (contextual by tab):**

- Pending: Accept (green) + Reject (red)
- All/Online: Remove (red, with confirmation)
- Blocked: Unblock (future)

### index.ts

Barrel export for `FriendsList` and `AddFriend`.

## Friend States

### Friendship Status

- **Pending (Outgoing)** - User sent request, awaiting response
- **Pending (Incoming)** - Received request, awaiting accept/reject
- **Accepted** - Active friendship
- **Blocked** - User blocked (one-way)

### Online Status

- **Online** - Currently connected
- **Offline** - Not connected
- **Away** - Idle (future)
- **DND** - Do Not Disturb (future)

## State Management

### From Stores

- `friendsState.friends` - Accepted friends list
- `friendsState.pendingRequests` - Incoming requests
- `friendsState.blocked` - Blocked users
- `friendsState.isLoading` - Loading state
- `getOnlineFriends()` - Filtered online friends

### Store Actions

- `loadFriends()` - Fetch friends list
- `loadPendingRequests()` - Fetch pending requests
- `loadBlocked()` - Fetch blocked users
- `acceptFriendRequest(friendshipId)` - Accept request
- `rejectFriendRequest(friendshipId)` - Reject request
- `removeFriend(friendshipId)` - Remove friend
- `sendFriendRequest(username)` - Send new request
- `blockUser(userId)` - Block user (future)
- `unblockUser(userId)` - Unblock user (future)

## Integration Points

### Backend APIs

- `GET /friends` - List friends
- `GET /friends/pending` - List pending requests
- `GET /friends/blocked` - List blocked users
- `POST /friends` - Send friend request
- `PATCH /friends/:id` - Accept request
- `DELETE /friends/:id` - Reject/remove friend
- `POST /users/:id/block` - Block user

### WebSocket Events

- `friend.online` - Friend came online
- `friend.offline` - Friend went offline
- `friend.request` - New friend request received
- `friend.accepted` - Request accepted by recipient
- `friend.removed` - Friendship ended

### Components

- `Avatar` (from `@/components/ui`) - User avatars
- `StatusIndicator` (from `@/components/ui`) - Online status

## Friend Discovery

### Current Method

- Manual username entry via AddFriend modal

### Future Methods

- Mutual server members
- Suggested friends (algorithm-based)
- Friend codes (shareable links)
- Import from other platforms

## Privacy Considerations

### Friend Requests

- Rate limiting (prevent spam)
- Block list prevents requests
- Optional: require mutual servers

### Online Status

- User can hide online status (future)
- Custom status messages
- Last seen timestamp (optional)

## UX Patterns

### Tab Counts

Display counts in tab labels:

- "Online (5)" - Active friends count
- "Pending (2)" - Awaiting action count

### Confirmation Dialogs

- Remove friend: "Are you sure you want to remove this friend?"
- Block user: "Are you sure you want to block {username}?"

### Hover States

- Friend rows highlight on hover
- Action buttons appear on hover (future enhancement)

## Styling

### Tab Bar

- Active tab: `bg-accent-primary text-surface-base`
- Inactive tab: `text-text-secondary hover:text-text-primary hover:bg-white/5`
- "Add Friend" button aligned right

### Friend Row

- Padding: `p-3`
- Hover: `hover:bg-white/5`
- Rounded: `rounded-lg`

### Action Buttons

- Accept: `bg-green-600 hover:bg-green-700`
- Reject/Remove: `bg-red-600 hover:bg-red-700`
- Small size: `px-3 py-1.5 text-sm`

### Avatar

- Size: 40px (w-10 h-10)
- Online indicator: 12px green dot

## Performance

### Optimizations

- Parallel data loading on mount
- Memoized filtered lists (online friends)
- Reactive counts (no manual updates)

### Considerations

- Large friend lists (>100): Virtualize rows
- Real-time updates: WebSocket for status changes
- Avatar caching: Cache avatar URLs

## Future Enhancements

- Friend notes (private notes about friends)
- Friend categories/groups
- Favorite friends
- Mutual friends display
- Friend activity feed
- Rich presence (game/app status)
- Profile viewer (click friend → view profile)

## Related Documentation

- Friends system: `PROJECT_SPEC.md` § Social Features
- WebSocket events: `STANDARDS.md` § WebSocket Protocol
- User profiles: `docs/user-profiles.md` (if exists)
