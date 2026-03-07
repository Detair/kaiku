# Floki Branding Client Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add Floki mascot illustrations to auth screens (split layout), onboarding wizard steps, home view, and emotes to empty states throughout the client.

**Architecture:** Image assets are already in `client/src/assets/images/` and `client/src/assets/emotes/`. Tasks modify existing JSX components to add `<img>` tags and adjust layouts. Auth screens get a two-column split layout with responsive fallback. No new components needed.

**Tech Stack:** Solid.js JSX, UnoCSS utility classes, static PNG imports via Vite

---

### Task 1: Commit image assets

**Files:**
- Stage: `client/src/assets/images/*.png` (10 files)
- Stage: `client/src/assets/emotes/*.png` (5 files)

**Step 1: Commit all image assets**

```bash
cd /home/detair/GIT/detair/kaiku/.claude/worktrees/floki-branding
git add client/src/assets/images/ client/src/assets/emotes/
git commit -m "chore(client): add Floki branding image assets

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 2: Auth — Login screen split layout

**Files:**
- Modify: `client/src/views/Login.tsx`

**Step 1: Add image import**

At the top of the file, after the existing imports, add:

```typescript
import flokiWelcome from "@/assets/images/floki_auth_welcome.png";
```

**Step 2: Wrap the return JSX in a two-column layout**

Replace the outer container (line 190):

```tsx
<div class="flex items-center justify-center min-h-screen bg-background-primary">
  <div class="w-full max-w-md p-8 bg-background-secondary rounded-lg shadow-lg">
```

With:

```tsx
<div class="flex items-center justify-center min-h-screen bg-background-primary">
  <div class="flex w-full max-w-4xl mx-4 bg-background-secondary rounded-lg shadow-lg overflow-hidden">
    {/* Left: Floki illustration */}
    <div class="hidden lg:flex w-1/2 items-center justify-center p-8 bg-surface-base">
      <img src={flokiWelcome} alt="Floki welcomes you" class="w-full max-w-xs object-contain" loading="lazy" />
    </div>
    {/* Right: Form */}
    <div class="w-full lg:w-1/2 p-8">
```

And close the extra divs at the bottom (line 424-425). The existing closing `</div></div>` becomes `</div></div></div>`.

**Step 3: Verify the form content is unchanged**

The form content between the `<div class="w-full lg:w-1/2 p-8">` wrapper should be identical to the original — just the header text, server URL input, MFA step, login form, and register link.

**Step 4: Commit**

```bash
git add client/src/views/Login.tsx
git commit -m "feat(client): add Floki illustration to login screen

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 3: Auth — Register screen split layout

**Files:**
- Modify: `client/src/views/Register.tsx`

**Step 1: Add image import**

```typescript
import flokiRegister from "@/assets/images/floki_auth_register.png";
```

**Step 2: Same two-column wrapper as Login**

Replace the outer container (line 187):

```tsx
<div class="flex items-center justify-center min-h-screen bg-background-primary py-8">
  <div class="w-full max-w-md p-8 bg-background-secondary rounded-lg shadow-lg">
```

With:

```tsx
<div class="flex items-center justify-center min-h-screen bg-background-primary py-8">
  <div class="flex w-full max-w-4xl mx-4 bg-background-secondary rounded-lg shadow-lg overflow-hidden">
    <div class="hidden lg:flex w-1/2 items-center justify-center p-8 bg-surface-base">
      <img src={flokiRegister} alt="Floki holding membership badge" class="w-full max-w-xs object-contain" loading="lazy" />
    </div>
    <div class="w-full lg:w-1/2 p-8">
```

Close the extra div at the end (before the component's final `</div></div>`).

**Step 3: Commit**

```bash
git add client/src/views/Register.tsx
git commit -m "feat(client): add Floki illustration to register screen

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 4: Auth — Forgot Password and Reset Password

**Files:**
- Modify: `client/src/views/ForgotPassword.tsx`
- Modify: `client/src/views/ResetPassword.tsx`

**Step 1: ForgotPassword — add image import and split layout**

Import:
```typescript
import flokiForgot from "@/assets/images/floki_auth_forgot.png";
```

Replace the outer container (line 56):
```tsx
<div class="flex items-center justify-center min-h-screen bg-background-primary">
  <div class="w-full max-w-md p-8 bg-background-secondary rounded-lg shadow-lg">
```

With the same two-column pattern using `flokiForgot` and alt text `"Floki turning a combination lock"`.

**Step 2: ResetPassword — same pattern**

Import:
```typescript
import flokiForgot from "@/assets/images/floki_auth_forgot.png";
```

Replace the outer container (line 68) with the same two-column pattern.

**Step 3: Commit**

```bash
git add client/src/views/ForgotPassword.tsx client/src/views/ResetPassword.tsx
git commit -m "feat(client): add Floki illustration to password reset screens

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 5: Onboarding Wizard — add images to each step

**Files:**
- Modify: `client/src/components/OnboardingWizard.tsx`

**Step 1: Add image imports**

After the existing imports (before `const MicTestPanel`):

```typescript
import flokiWelcome from "@/assets/images/floki_onboarding_welcome.png";
import flokiTheme from "@/assets/images/floki_onboard_theme.png";
import flokiMic from "@/assets/images/floki_onboard_mic.png";
import flokiJoin from "@/assets/images/floki_onboard_join.png";
import flokiDone from "@/assets/images/floki_onboard_done.png";
```

**Step 2: Step 0 (Welcome) — add image above heading**

Replace the text-center div at line 311:

```tsx
<div class="text-center mb-6">
  <h2 class="text-2xl font-bold text-text-primary">
    Welcome to Kaiku
  </h2>
  <p class="text-sm text-text-secondary mt-2">
    Let's get you set up in just a few steps.
  </p>
</div>
```

With:

```tsx
<div class="text-center mb-6">
  <img src={flokiWelcome} alt="Floki waving hello" class="w-24 h-24 mx-auto mb-3 object-contain" loading="lazy" />
  <h2 class="text-2xl font-bold text-text-primary">
    Welcome to Kaiku
  </h2>
  <p class="text-sm text-text-secondary mt-2">
    Let's get you set up in just a few steps.
  </p>
</div>
```

**Step 3: Step 1 (Theme) — replace text-center header**

Replace the header at line 345:

```tsx
<div class="text-center mb-6">
  <h2 class="text-xl font-bold text-text-primary">
    Pick a Theme
  </h2>
  <p class="text-sm text-text-secondary mt-1">
    Choose how Kaiku looks. You can change this anytime.
  </p>
</div>
```

With:

```tsx
<div class="text-center mb-6">
  <img src={flokiTheme} alt="Floki painting with colors" class="w-20 h-20 mx-auto mb-3 object-contain" loading="lazy" />
  <h2 class="text-xl font-bold text-text-primary">
    Pick a Theme
  </h2>
  <p class="text-sm text-text-secondary mt-1">
    Choose how Kaiku looks. You can change this anytime.
  </p>
</div>
```

**Step 4: Step 2 (Mic) — replace the Mic icon**

Replace lines 399-405:

```tsx
<div class="text-center mb-4">
  <Mic class="w-8 h-8 text-accent-primary mx-auto mb-2" />
  <h2 class="text-xl font-bold text-text-primary">Mic Setup</h2>
  <p class="text-sm text-text-secondary mt-1">
    Test your microphone and speakers. You can skip this step.
  </p>
</div>
```

With:

```tsx
<div class="text-center mb-4">
  <img src={flokiMic} alt="Floki with gaming headset" class="w-20 h-20 mx-auto mb-2 object-contain" loading="lazy" />
  <h2 class="text-xl font-bold text-text-primary">Mic Setup</h2>
  <p class="text-sm text-text-secondary mt-1">
    Test your microphone and speakers. You can skip this step.
  </p>
</div>
```

**Step 5: Step 3 (Join) — replace the Compass icon**

Replace lines 419-427:

```tsx
<div class="text-center mb-4">
  <Compass class="w-8 h-8 text-accent-primary mx-auto mb-2" />
  <h2 class="text-xl font-bold text-text-primary">
    Join a Server
  </h2>
  <p class="text-sm text-text-secondary mt-1">
    Find a community or enter an invite code.
  </p>
</div>
```

With:

```tsx
<div class="text-center mb-4">
  <img src={flokiJoin} alt="Floki exploring server islands" class="w-20 h-20 mx-auto mb-2 object-contain" loading="lazy" />
  <h2 class="text-xl font-bold text-text-primary">
    Join a Server
  </h2>
  <p class="text-sm text-text-secondary mt-1">
    Find a community or enter an invite code.
  </p>
</div>
```

**Step 6: Step 4 (Done) — replace the checkmark circle**

Replace lines 611-622:

```tsx
<div class="text-center py-6">
  <div class="w-16 h-16 rounded-full bg-accent-primary/20 flex items-center justify-center mx-auto mb-4">
    <Check class="w-8 h-8 text-accent-primary" />
  </div>
  <h2 class="text-2xl font-bold text-text-primary">
    You're All Set!
  </h2>
  <p class="text-sm text-text-secondary mt-2 max-w-xs mx-auto">
    Welcome aboard, {currentUser()?.display_name ?? "friend"}.
    Explore servers, chat with friends, and join voice channels.
  </p>
</div>
```

With:

```tsx
<div class="text-center py-6">
  <img src={flokiDone} alt="Floki celebrating" class="w-28 h-28 mx-auto mb-4 object-contain" loading="lazy" />
  <h2 class="text-2xl font-bold text-text-primary">
    You're All Set!
  </h2>
  <p class="text-sm text-text-secondary mt-2 max-w-xs mx-auto">
    Welcome aboard, {currentUser()?.display_name ?? "friend"}.
    Explore servers, chat with friends, and join voice channels.
  </p>
</div>
```

**Step 7: Clean up unused icon imports**

Remove `Mic` and `Compass` from the lucide-solid import if they are no longer used elsewhere in the file. `Check` is still used in the theme step, so keep it. `ChevronRight`, `ChevronLeft`, and `Users` are still used.

Updated import:
```typescript
import {
  Check,
  ChevronRight,
  ChevronLeft,
  Users,
} from "lucide-solid";
```

**Step 8: Commit**

```bash
git add client/src/components/OnboardingWizard.tsx
git commit -m "feat(client): add Floki illustrations to onboarding wizard steps

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 6: Home view — idle dashboard illustration

**Files:**
- Modify: `client/src/components/home/HomeView.tsx`

**Step 1: Add image import**

```typescript
import flokiHome from "@/assets/images/floki_home_idle.png";
```

**Step 2: Read the current HomeView**

The current HomeView shows FriendsList or DMConversation. We need to add a Floki illustration when the home view first loads and no DM is selected. Check `dmsState` — when `isShowingFriends` is true and there's no active DM, the FriendsList shows. The idle state image should appear in the FriendsList area if there are no friends, OR as a welcome banner at the top.

The simplest approach: when `isShowingFriends` is true, show a small Floki image above the FriendsList as a welcome header in the main content area.

Actually, to keep it simple and avoid touching FriendsList internals, add the Floki idle image as a centered placeholder when no DM conversation is active and friends list is NOT showing (i.e., no DM selected, no friends tab active — this happens when the home view loads without a specific view).

Looking at the code: `dmsState.isShowingFriends` defaults to true when Home is selected. So the FriendsList is always shown. Let's instead put the Floki home image in the `HomeRightPanel` or as a banner.

Simplest: add the Floki home illustration as a subtle centered element in HomeView when friends list is shown (above or inside it). But touching FriendsList is complex.

**Better approach:** Replace the `HomeRightPanel` empty state or add the illustration to the HomeView as a fallback when no DM is active. Read HomeRightPanel first.

Actually, the simplest approach that matches the design: show the Floki home idle illustration as a centered element in the main stage when `isShowingFriends` is true, above the FriendsList. This means modifying HomeView.tsx to wrap the FriendsList with a header image.

Replace lines 14-21 of HomeView.tsx:

```tsx
<div class="flex-1 flex h-full">
  <div class="flex-1 flex flex-col min-w-0">
    <Show when={dmsState.isShowingFriends} fallback={<DMConversation />}>
      <FriendsList />
    </Show>
  </div>
  <HomeRightPanel />
</div>
```

With:

```tsx
<div class="flex-1 flex h-full">
  <div class="flex-1 flex flex-col min-w-0">
    <Show when={dmsState.isShowingFriends} fallback={<DMConversation />}>
      <div class="flex flex-col items-center pt-8 pb-4">
        <img src={flokiHome} alt="Floki relaxing at desk" class="w-32 h-32 object-contain opacity-80" loading="lazy" />
        <p class="text-sm text-text-secondary mt-2">Welcome home</p>
      </div>
      <FriendsList />
    </Show>
  </div>
  <HomeRightPanel />
</div>
```

Note: The `<Show>` children need to be wrapped in a fragment or container since Solid.js `Show` expects a single child. Wrap in a fragment `<>...</>`:

```tsx
<Show when={dmsState.isShowingFriends} fallback={<DMConversation />}>
  <>
    <div class="flex flex-col items-center pt-8 pb-4">
      <img src={flokiHome} alt="Floki relaxing at desk" class="w-32 h-32 object-contain opacity-80" loading="lazy" />
      <p class="text-sm text-text-secondary mt-2">Welcome home</p>
    </div>
    <FriendsList />
  </>
</Show>
```

**Step 3: Commit**

```bash
git add client/src/components/home/HomeView.tsx
git commit -m "feat(client): add Floki idle illustration to home view

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 7: Empty states — emotes for MessageList, DiscoveryView, SearchPanel

**Files:**
- Modify: `client/src/components/messages/MessageList.tsx:356-371`
- Modify: `client/src/components/discovery/DiscoveryView.tsx:191-200`
- Modify: `client/src/components/search/SearchPanel.tsx:361-365`

**Step 1: MessageList — replace emoji with Floki emote**

Add import at top:
```typescript
import flokiHappy from "@/assets/emotes/floki_emote_1.png";
```

Replace the empty state (lines 360-370):

```tsx
<div class="flex flex-col items-center justify-center h-full text-center px-4">
  <div class="w-20 h-20 bg-surface-layer2 rounded-full flex items-center justify-center mb-4">
    <span class="text-4xl">👋</span>
  </div>
  <h3 class="text-lg font-semibold text-text-primary mb-2">
    No messages yet
  </h3>
  <p class="text-text-secondary max-w-sm">
    Be the first to send a message in this channel!
  </p>
</div>
```

With:

```tsx
<div class="flex flex-col items-center justify-center h-full text-center px-4">
  <img src={flokiHappy} alt="" class="w-16 h-16 object-contain mb-4" loading="lazy" />
  <h3 class="text-lg font-semibold text-text-primary mb-2">
    No messages yet
  </h3>
  <p class="text-text-secondary max-w-sm">
    Be the first to send a message in this channel!
  </p>
</div>
```

**Step 2: DiscoveryView — replace Search icon with Floki emote**

Add import:
```typescript
import flokiThinking from "@/assets/emotes/floki_emote_2.png";
```

Replace the empty state (lines 192-199):

```tsx
<div class="flex flex-col items-center justify-center py-16 text-center">
  <Search class="w-10 h-10 text-text-secondary opacity-30 mb-3" />
  <p class="text-text-secondary text-sm">
    {query()
      ? "No servers found matching your search."
      : "No discoverable servers yet."}
  </p>
</div>
```

With:

```tsx
<div class="flex flex-col items-center justify-center py-16 text-center">
  <img src={flokiThinking} alt="" class="w-16 h-16 object-contain mb-3" loading="lazy" />
  <p class="text-text-secondary text-sm">
    {query()
      ? "No servers found matching your search."
      : "No discoverable servers yet."}
  </p>
</div>
```

**Step 3: SearchPanel — replace Search icon with Floki emote**

Add import:
```typescript
import flokiThinking from "@/assets/emotes/floki_emote_2.png";
```

Replace the "No results found" empty state (lines 361-365):

```tsx
<div class="flex flex-col items-center justify-center py-8 text-text-secondary">
  <Search class="w-12 h-12 mb-3 opacity-50" />
  <p class="text-sm">No results found</p>
  <p class="text-xs mt-1">Try different keywords</p>
</div>
```

With:

```tsx
<div class="flex flex-col items-center justify-center py-8 text-text-secondary">
  <img src={flokiThinking} alt="" class="w-14 h-14 object-contain mb-3" loading="lazy" />
  <p class="text-sm">No results found</p>
  <p class="text-xs mt-1">Try different keywords</p>
</div>
```

**Step 4: Commit**

```bash
git add client/src/components/messages/MessageList.tsx client/src/components/discovery/DiscoveryView.tsx client/src/components/search/SearchPanel.tsx
git commit -m "feat(client): add Floki emotes to message, discovery, and search empty states

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 8: Empty states — emotes for HomeSidebar, ChannelList, home modules

**Files:**
- Modify: `client/src/components/home/HomeSidebar.tsx:139-142`
- Modify: `client/src/components/channels/ChannelList.tsx:642-644`
- Modify: `client/src/components/home/modules/UnreadModule.tsx:313-317`
- Modify: `client/src/components/home/modules/PinsModule.tsx:229-236`

**Step 1: HomeSidebar — "No conversations" emote**

Add import:
```typescript
import flokiThinking from "@/assets/emotes/floki_emote_2.png";
```

Replace (lines 139-142):
```tsx
<div class="text-center py-8 px-4">
  <p class="text-text-secondary text-sm">
    No conversations yet
  </p>
```

With:
```tsx
<div class="text-center py-8 px-4">
  <img src={flokiThinking} alt="" class="w-12 h-12 mx-auto mb-2 object-contain" loading="lazy" />
  <p class="text-text-secondary text-sm">
    No conversations yet
  </p>
```

**Step 2: ChannelList — "No channels" emote**

Add import:
```typescript
import flokiHappy from "@/assets/emotes/floki_emote_1.png";
```

Replace (lines 642-644):
```tsx
<div class="px-2 py-4 text-center text-text-secondary text-sm">
  No channels yet
</div>
```

With:
```tsx
<div class="px-2 py-4 text-center text-text-secondary text-sm">
  <img src={flokiHappy} alt="" class="w-10 h-10 mx-auto mb-1 object-contain" loading="lazy" />
  No channels yet
</div>
```

**Step 3: UnreadModule — "All caught up" emote**

Add import:
```typescript
import flokiParty from "@/assets/emotes/floki_emote_3.png";
```

Replace (lines 313-317):
```tsx
<div class="flex flex-col items-center justify-center py-4 text-center">
  <Inbox class="w-8 h-8 text-text-secondary mb-2 opacity-50" />
  <p class="text-sm text-text-secondary">All caught up!</p>
  <p class="text-xs text-text-muted mt-1">No unread messages</p>
</div>
```

With:
```tsx
<div class="flex flex-col items-center justify-center py-4 text-center">
  <img src={flokiParty} alt="" class="w-12 h-12 mb-2 object-contain" loading="lazy" />
  <p class="text-sm text-text-secondary">All caught up!</p>
  <p class="text-xs text-text-muted mt-1">No unread messages</p>
</div>
```

**Step 4: PinsModule — "No pins" emote**

Add import:
```typescript
import flokiCool from "@/assets/emotes/floki_emote_4.png";
```

Replace (lines 229-236):
```tsx
<Show when={pins().length === 0 && !isAdding()}>
  <div class="text-center py-4">
    <Pin class="w-8 h-8 text-text-secondary mx-auto mb-2 opacity-50" />
    <p class="text-sm text-text-secondary">No pins yet</p>
    <p class="text-xs text-text-muted mt-1">
      Save notes and links for quick access
    </p>
  </div>
</Show>
```

With:
```tsx
<Show when={pins().length === 0 && !isAdding()}>
  <div class="text-center py-4">
    <img src={flokiCool} alt="" class="w-10 h-10 mx-auto mb-2 object-contain" loading="lazy" />
    <p class="text-sm text-text-secondary">No pins yet</p>
    <p class="text-xs text-text-muted mt-1">
      Save notes and links for quick access
    </p>
  </div>
</Show>
```

**Step 5: Commit**

```bash
git add client/src/components/home/HomeSidebar.tsx client/src/components/channels/ChannelList.tsx client/src/components/home/modules/UnreadModule.tsx client/src/components/home/modules/PinsModule.tsx
git commit -m "feat(client): add Floki emotes to sidebar and module empty states

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 9: CHANGELOG and verify

**Files:**
- Modify: `CHANGELOG.md`

**Step 1: Add changelog entry**

Under `[Unreleased]` > `### Added`, add:

```markdown
- Floki mascot branding throughout the client — illustrations on login, register, and password reset screens (side-by-side layout), per-step illustrations in onboarding wizard, idle dashboard image on home view, and Floki emotes replacing generic icons in empty states
```

**Step 2: Run tests**

```bash
cd client && bun run test:run
```

Expected: All tests pass (changes are purely visual — JSX img tags and layout classes).

**Step 3: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs(client): add changelog entry for Floki branding

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```
