# Floki Branding & Icons Across the Client UI

**Date:** 2026-03-07
**Status:** Approved
**Scope:** Floki mascot images and emotes throughout the Kaiku client

## Goal

Add Floki-branded illustrations and emote icons throughout the client UI at three tiers: full illustrations for high-impact screens (auth, onboarding, home), emote-style heads for minor empty states, and the existing logo for navigation.

## Style Guide Reference

All new images follow the golden base prompt from `docs/developer-guide/design/image-generation-guidelines.md`:

> A sleek, modern stylized vector illustration of a fluffy Suomenlapinkoira (Finnish Lapphund) dog. The art style is premium gaming aesthetic, using a CachyOS Nordic color palette: dark charcoal and slate backgrounds, accented with vibrant frosty cyan and glowing aurora purple. Clean lines, subtle neon glow, glassmorphism elements, flat design with depth.

## Tier 1: Full Illustrations (10 New Assets)

### 1.1 Auth Screens -- Side-by-Side Layout

All 4 auth screens (Login, Register, Forgot Password, Reset Password) use a split layout: Floki illustration on the left, form on the right. On narrow viewports the illustration hides and the form goes full-width.

#### `floki_auth_welcome.png` -- Login

> `[BASE PROMPT] The dog is sitting confidently in front of a glowing doorway portal made of cyan light, looking welcoming and friendly, one paw raised in greeting. A subtle keyhole shape glows in the portal center. Highly detailed, minimalist background, transparent background feel.`

#### `floki_auth_register.png` -- Register

> `[BASE PROMPT] The dog is excitedly holding up a glowing cyan ID badge or membership card, tail wagging, looking proud and eager to join. A faint aurora purple sparkle trail follows the card. Highly detailed, minimalist background, transparent background feel.`

#### `floki_auth_forgot.png` -- Forgot Password / Reset Password

> `[BASE PROMPT] The dog is carefully turning a glowing combination lock, concentrating with tongue slightly out, a floating holographic key materializing nearby. Highly detailed, minimalist background, transparent background feel.`

### 1.2 Onboarding Wizard -- 5 Steps

#### `floki_onboard_welcome.png` -- Step 1: Welcome / Display Name

> `[BASE PROMPT] The dog is waving enthusiastically with both paws, standing on a glowing cyan welcome mat, a floating holographic name tag hovering beside him ready to be filled in. Highly detailed, minimalist background, transparent background feel.`

#### `floki_onboard_theme.png` -- Step 2: Theme Selection

> `[BASE PROMPT] The dog is holding a glowing paint roller, painting the air with streaks of cyan and purple light, surrounded by floating color palette swatches in a semicircle. Highly detailed, minimalist background, transparent background feel.`

#### `floki_onboard_mic.png` -- Step 3: Mic Setup

> `[BASE PROMPT] The dog is wearing a sleek gaming headset with glowing cyan ear cups and an aurora purple mic boom, testing the microphone with sound wave rings emanating outward. Highly detailed, minimalist background, transparent background feel.`

#### `floki_onboard_join.png` -- Step 4: Join a Server

> `[BASE PROMPT] The dog is looking through a glowing cyan telescope or spyglass at distant floating server islands connected by aurora light bridges, tail wagging with excitement. Highly detailed, minimalist background, transparent background feel.`

#### `floki_onboard_done.png` -- Step 5: All Set

> `[BASE PROMPT] The dog is celebrating with a triumphant howl, confetti made of tiny cyan and purple geometric shapes raining down, a glowing checkmark circle behind him. Highly detailed, minimalist background, transparent background feel.`

### 1.3 Home View

#### `floki_home_idle.png` -- Dashboard landing (no guild/DM selected)

> `[BASE PROMPT] The dog is relaxing at a cozy futuristic desk, leaning back comfortably, with softly glowing dual monitors showing chat interfaces, a warm cup of coffee nearby, looking content and at ease. Highly detailed, minimalist background, transparent background feel.`

### 1.4 Pending from Previous Batch

#### `floki_feat_safety.png` -- Content Filters & Safety

> `[BASE PROMPT] The dog is standing guard, holding a glowing purple and cyan energy shield that is blocking out negative spiky "spam" icons, symbolizing Content Filters and Safety. Highly detailed, minimalist background, transparent background feel.`

## Tier 2: Emote Reuse for Empty States

Existing emotes from `kaiku-landing/assets/images/extracted/` copied to `client/src/assets/emotes/` and displayed at ~64-80px.

| Empty State | File | Emote Style |
|---|---|---|
| No messages in channel | `floki_emote_1.png` | Happy / thumbs up |
| No search results | `floki_emote_2.png` | Thinking / confused |
| No friends found | `floki_emote_5.png` | Sad |
| No conversations (DMs) | `floki_emote_2.png` | Thinking / confused |
| No discoverable servers | `floki_emote_2.png` | Thinking / confused |
| No channels in guild | `floki_emote_1.png` | Happy / thumbs up |
| No pins / pages / bots | `floki_emote_4.png` | Cool / sunglasses |
| No unread messages | `floki_emote_3.png` | Party / celebrating |

## Tier 3: Logo

Existing `floki_logo_circle.png` in the ServerRail home button. No changes needed.

## Not Included

- Loading spinners/skeletons -- transient states, images would slow perceived performance
- Error states -- use existing error banner system with text
- Settings panels -- too many sub-sections, visual noise
- Admin dashboard -- data-dense UI, illustrations would clutter

## Implementation Notes

- New images stored in `client/src/assets/images/` (full illustrations) and `client/src/assets/emotes/` (emote heads)
- Auth screens need layout refactor from single centered card to split two-column
- Onboarding wizard needs image slot added above or beside each step's content
- Empty states replace current emoji/icon placeholders with `<img>` tags
- All images should have `loading="lazy"` and appropriate `alt` text for accessibility
