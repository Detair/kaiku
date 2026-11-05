/**
 * Marketing Screenshots — Full User Journey
 *
 * Walks through the complete Kaiku experience from registration
 * through all major features, capturing high-quality screenshots
 * at each milestone for marketing materials.
 *
 * Run: npx playwright test e2e/marketing-screenshots.spec.ts --headed
 */

import { test, expect, type Page } from "@playwright/test";

const SCREENSHOT_DIR = "screenshots";
const VIEWPORT = { width: 1440, height: 900 };

let screenshotIndex = 0;

async function screenshot(page: Page, name: string) {
  screenshotIndex += 1;
  const paddedIndex = String(screenshotIndex).padStart(2, "0");
  await page.screenshot({
    path: `${SCREENSHOT_DIR}/${paddedIndex}-${name}.png`,
    fullPage: false,
  });
}

async function waitAndScreenshot(page: Page, name: string, waitMs = 500) {
  await page.waitForTimeout(waitMs);
  await screenshot(page, name);
}

// ------------------------------------------------------------------
// Helpers for API calls
// ------------------------------------------------------------------

const BACKEND = "http://localhost:8080";

async function apiPost(endpoint: string, data: unknown, token?: string) {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (token) headers["Authorization"] = `Bearer ${token}`;
  const res = await fetch(`${BACKEND}${endpoint}`, {
    method: "POST",
    headers,
    body: JSON.stringify(data),
  });
  return res;
}

async function apiGet(endpoint: string, token: string) {
  const res = await fetch(`${BACKEND}${endpoint}`, {
    headers: { Authorization: `Bearer ${token}` },
  });
  return res;
}

async function apiPut(endpoint: string, data: unknown, token: string) {
  const res = await fetch(`${BACKEND}${endpoint}`, {
    method: "PUT",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${token}`,
    },
    body: JSON.stringify(data),
  });
  return res;
}

async function registerUser(
  username: string,
  password: string,
  displayName: string,
  email: string,
) {
  await apiPost("/auth/register", {
    username,
    password,
    display_name: displayName,
    email,
  });
}

async function loginApi(username: string, password: string): Promise<string> {
  const res = await apiPost("/auth/login", { username, password });
  const json = (await res.json()) as { access_token: string };
  return json.access_token;
}

// ------------------------------------------------------------------
// Test Suite
// ------------------------------------------------------------------

test.describe("Marketing Screenshots", () => {
  test.setTimeout(180_000);

  test.beforeAll(async ({ }, testInfo) => {
    testInfo.setTimeout(60_000);
    screenshotIndex = 0;

    // Seed test data via API
    // 1. Register users
    await registerUser("alex", "SecurePass123!", "Alex Morgan", "alex@kaiku.chat");
    await registerUser("sam", "SecurePass123!", "Sam Rivera", "sam@kaiku.chat");
    await registerUser("jordan", "SecurePass123!", "Jordan Chen", "jordan@kaiku.chat");
    await registerUser("taylor", "SecurePass123!", "Taylor Park", "taylor@kaiku.chat");

    // 2. Login as Alex (first user = admin) and complete setup
    const alexToken = await loginApi("alex", "SecurePass123!");

    // Complete first-run setup
    await apiPost(
      "/api/setup/complete",
      {
        server_name: "Kaiku Community",
        registration_policy: "open",
        terms_url: null,
        privacy_url: null,
      },
      alexToken,
    );

    // Mark onboarding completed for all users
    for (const [user, pass] of [
      ["alex", "SecurePass123!"],
      ["sam", "SecurePass123!"],
      ["jordan", "SecurePass123!"],
      ["taylor", "SecurePass123!"],
    ] as const) {
      const token = await loginApi(user, pass);
      await apiPut(
        "/api/me/preferences",
        { preferences: { onboarding_completed: true } },
        token,
      );
    }

    // 3. Create guilds
    const createGuild = async (name: string, description: string, token: string) => {
      const res = await apiPost("/api/guilds", { name, description }, token);
      const json = (await res.json()) as { id: string };
      return json.id;
    };

    const gamingGuildId = await createGuild(
      "Pixel Legends",
      "Gaming community for strategy and RPG enthusiasts. Weekly tournaments, clan wars, and chill sessions.",
      alexToken,
    );

    const devGuildId = await createGuild(
      "Code Forge",
      "Open-source developers building the future. Rust, TypeScript, and everything in between.",
      alexToken,
    );

    // 4. Create channels in gaming guild
    const createChannel = async (
      name: string,
      type: string,
      guildId: string,
      token: string,
    ) => {
      const res = await apiPost(
        "/api/channels",
        { name, channel_type: type, guild_id: guildId },
        token,
      );
      const json = (await res.json()) as { id: string };
      return json.id;
    };

    // Find the default "general" channel that was auto-created with the guild
    const getDefaultChannel = async (guildId: string, token: string): Promise<string> => {
      const res = await apiGet(`/api/guilds/${guildId}/channels`, token);
      const channels = (await res.json()) as Array<{ id: string; name: string; channel_type: string }>;
      console.log(`Guild ${guildId} channels:`, JSON.stringify(channels.map(c => ({ id: c.id, name: c.name, type: c.channel_type }))));
      const textChannel = channels.find((c) => c.channel_type === "text");
      if (textChannel) {
        console.log(`Using existing channel: ${textChannel.id} (${textChannel.name})`);
        return textChannel.id;
      }
      console.log("No text channel found, creating one");
      return createChannel("general", "text", guildId, token);
    };

    const generalChatId = await getDefaultChannel(gamingGuildId, alexToken);
    await createChannel("strategy-talk", "text", gamingGuildId, alexToken);
    await createChannel("off-topic", "text", gamingGuildId, alexToken);
    await createChannel("Voice Lounge", "voice", gamingGuildId, alexToken);
    await createChannel("Game Night", "voice", gamingGuildId, alexToken);

    // Dev guild channels
    await createChannel("rust-help", "text", devGuildId, alexToken);
    await createChannel("code-review", "text", devGuildId, alexToken);
    await createChannel("Pair Programming", "voice", devGuildId, alexToken);

    // 5. Create invite codes and have users join
    const createInvite = async (guildId: string, token: string) => {
      const res = await apiPost(
        `/api/guilds/${guildId}/invites`,
        { expires_in: "7d" },
        token,
      );
      const json = (await res.json()) as { code: string };
      return json.code;
    };

    const gamingInvite = await createInvite(gamingGuildId, alexToken);
    const devInvite = await createInvite(devGuildId, alexToken);

    // Have other users join guilds
    for (const [user, pass] of [
      ["sam", "SecurePass123!"],
      ["jordan", "SecurePass123!"],
      ["taylor", "SecurePass123!"],
    ] as const) {
      const token = await loginApi(user, pass);
      await apiPost(`/api/invites/${gamingInvite}/join`, {}, token);
      await apiPost(`/api/invites/${devInvite}/join`, {}, token);
    }

    // 6. Seed some messages in general chat
    const samToken = await loginApi("sam", "SecurePass123!");
    const jordanToken = await loginApi("jordan", "SecurePass123!");
    const taylorToken = await loginApi("taylor", "SecurePass123!");

    const sendMessage = async (channelId: string, content: string, token: string) => {
      const res = await apiPost(`/api/messages/channel/${channelId}`, { content }, token);
      if (!res.ok) {
        const body = await res.text();
        console.error(`sendMessage failed [${res.status}]: ${body} (channel: ${channelId})`);
      }
    };

    await sendMessage(generalChatId, "Hey everyone! Ready for game night? 🎮", alexToken);
    await sendMessage(
      generalChatId,
      "Count me in! What are we playing tonight?",
      samToken,
    );
    await sendMessage(
      generalChatId,
      "I vote for Civilization. We had an amazing match last week!",
      jordanToken,
    );
    await sendMessage(
      generalChatId,
      "Civ sounds great! I've been practicing my rush strategy 😄",
      taylorToken,
    );
    await sendMessage(
      generalChatId,
      "Tournament bracket is posted in #strategy-talk. Check it out!",
      alexToken,
    );
    await sendMessage(
      generalChatId,
      "Nice! Also, who's joining voice chat? I'll hop in the lounge in 10 min",
      samToken,
    );
    await sendMessage(
      generalChatId,
      "I'll be there! Let me grab my headset",
      jordanToken,
    );
    await sendMessage(
      generalChatId,
      "This community is awesome. So glad I found Kaiku — way better than Discord for privacy!",
      taylorToken,
    );

    // 7. Send friend requests between users (best-effort)
    try {
      await apiPost("/api/friends/request", { username: "sam" }, alexToken);
      await apiPost("/api/friends/request", { username: "jordan" }, alexToken);

      // Accept friend requests
      for (const [token] of [
        [samToken],
        [jordanToken],
      ] as const) {
        const res = await apiGet("/api/friends/requests/incoming", token);
        if (res.ok) {
          const text = await res.text();
          if (text) {
            const requests = JSON.parse(text) as Array<{
              id: string;
              sender: { username: string };
            }>;
            for (const req of requests) {
              await apiPost(`/api/friends/requests/${req.id}/accept`, {}, token);
            }
          }
        }
      }
    } catch {
      // Friend system may not be fully available — continue without it
    }
  });

  test("full user journey", async ({ browser }) => {
    const context = await browser.newContext({
      viewport: VIEWPORT,
      ignoreHTTPSErrors: true,
    });
    const page = await context.newPage();

    // ===== 1. LOGIN PAGE =====
    await page.goto("https://localhost:5173/login");
    await page.waitForLoadState("networkidle");
    await waitAndScreenshot(page, "login-page", 1000);

    // ===== 2. REGISTER PAGE =====
    // Click the register link
    const registerLink = page.getByRole("link", { name: /register|sign up|create/i });
    if (await registerLink.isVisible({ timeout: 2000 }).catch(() => false)) {
      await registerLink.click();
      await page.waitForLoadState("networkidle");
      await waitAndScreenshot(page, "register-page", 1000);
    }

    // ===== 3. LOGIN AS ALEX =====
    await page.goto("https://localhost:5173/login");
    await page.waitForLoadState("networkidle");
    await page.fill('input[placeholder="Enter your username"]', "alex");
    await page.getByTestId("login-show-password").click();
    await page.fill('input[placeholder="Enter your password"]', "SecurePass123!");
    await waitAndScreenshot(page, "login-filled");
    await page.click('button[type="submit"]');

    // Handle first-run setup if visible
    const setupHeading = page.getByRole("heading", { name: /Welcome to Kaiku/i });
    const setupVisible = await setupHeading
      .isVisible({ timeout: 3000 })
      .catch(() => false);
    if (setupVisible) {
      await waitAndScreenshot(page, "first-run-setup", 1000);
      const completeButton = page.locator('button:has-text("Complete Setup")').first();
      await expect(completeButton).toBeVisible({ timeout: 5000 });
      await completeButton.click();
      await expect(setupHeading).toBeHidden({ timeout: 15000 });
    }

    // Handle onboarding wizard if visible
    const onboardingDialog = page.getByRole("dialog", {
      name: "Onboarding wizard",
    });
    const onboardingVisible = await onboardingDialog
      .isVisible({ timeout: 3000 })
      .catch(() => false);
    if (onboardingVisible) {
      await waitAndScreenshot(page, "onboarding-wizard", 1000);
      // Click through onboarding
      for (let step = 0; step < 6; step++) {
        const getStarted = onboardingDialog.getByRole("button", {
          name: "Get Started",
        });
        if (await getStarted.isVisible({ timeout: 500 }).catch(() => false)) {
          await getStarted.click();
          break;
        }
        const cont = onboardingDialog.getByRole("button", { name: "Continue" });
        if (await cont.isVisible({ timeout: 500 }).catch(() => false)) {
          await cont.click();
          continue;
        }
        const next = onboardingDialog.getByRole("button", { name: "Next" });
        if (await next.isVisible({ timeout: 500 }).catch(() => false)) {
          await next.click();
          continue;
        }
        const skip = onboardingDialog.getByRole("button", { name: "Skip" });
        if (await skip.isVisible({ timeout: 500 }).catch(() => false)) {
          await skip.click();
          continue;
        }
      }
      await expect(onboardingDialog).toBeHidden({ timeout: 15000 });
    }

    // Wait for the app to fully load
    await expect(page.locator("aside").first()).toBeVisible({ timeout: 15000 });
    await page.waitForTimeout(1500);

    // ===== 4. HOME VIEW =====
    await waitAndScreenshot(page, "home-view", 1000);

    // ===== 5. SELECT GAMING GUILD =====
    const pixelLegendsBtn = page.locator('button[title="Pixel Legends"]').first();
    if (await pixelLegendsBtn.isVisible({ timeout: 5000 }).catch(() => false)) {
      await pixelLegendsBtn.click();
    } else {
      // Click first guild if specific button not found
      const guildButtons = page
        .locator("aside")
        .first()
        .locator(
          'button[title]:not([title="Home"]):not([title="Explore Servers"]):not([title="Create Server"]):not([title="Join Server"])',
        );
      await guildButtons.first().click();
    }
    await page.waitForTimeout(1000);

    // ===== 6. GUILD WITH CHANNELS =====
    await waitAndScreenshot(page, "guild-channel-list", 1000);

    // ===== 7. CLICK GENERAL CHANNEL — SHOW CHAT =====
    const generalChannel = page
      .locator("aside")
      .nth(1)
      .locator('[role="button"]')
      .filter({ hasText: /general/i })
      .first();
    if (await generalChannel.isVisible({ timeout: 5000 }).catch(() => false)) {
      await generalChannel.click();
    }
    // Wait for messages to load (historical messages from API seed)
    await page.waitForTimeout(3000);
    await waitAndScreenshot(page, "chat-messages", 500);

    // ===== 8. TYPE A MESSAGE =====
    const messageInput = page.locator(
      '[data-testid="message-input"], textarea[placeholder*="message" i], div[contenteditable="true"]',
    ).first();
    if (await messageInput.isVisible({ timeout: 3000 }).catch(() => false)) {
      await messageInput.click();
      // Use keyboard.type instead of fill to work better with contenteditable
      await page.keyboard.type("Who else is hyped for the tournament this weekend? Let's go!", { delay: 20 });
      await waitAndScreenshot(page, "chat-composing", 500);

      // Send the message
      await page.keyboard.press("Enter");
      await page.waitForTimeout(1500);
      // Dismiss any error toasts by clicking them or waiting
      const toast = page.locator('[class*="toast"], [role="alert"]').first();
      if (await toast.isVisible({ timeout: 500 }).catch(() => false)) {
        await toast.click().catch(() => {});
        await page.waitForTimeout(500);
      }
      await waitAndScreenshot(page, "chat-message-sent", 500);
    }

    // ===== 9. EMOJI REACTION (right-click a message) =====
    const lastMessage = page.locator('[data-testid="message-item"]').last();
    if (await lastMessage.isVisible({ timeout: 3000 }).catch(() => false)) {
      await lastMessage.hover();
      await page.waitForTimeout(500);
      // Look for reaction button on hover
      const reactButton = page
        .locator('[data-testid="message-action-react"], button[title*="React" i]')
        .first();
      if (await reactButton.isVisible({ timeout: 2000 }).catch(() => false)) {
        await reactButton.click();
        await page.waitForTimeout(500);
        await waitAndScreenshot(page, "emoji-picker", 500);
        // Click an emoji
        const emoji = page.locator('[data-testid="emoji-picker"] button').first();
        if (await emoji.isVisible({ timeout: 2000 }).catch(() => false)) {
          await emoji.click();
          await page.waitForTimeout(500);
        } else {
          // Close the picker
          await page.keyboard.press("Escape");
        }
      }
    }

    // ===== 10. THREAD (open from message) =====
    const threadSourceMessage = page.locator('[data-testid="message-item"]').nth(2);
    if (await threadSourceMessage.isVisible({ timeout: 2000 }).catch(() => false)) {
      await threadSourceMessage.hover();
      await page.waitForTimeout(300);
      const threadButton = page
        .locator('[data-testid="message-action-thread"], button[title*="Thread" i]')
        .first();
      if (await threadButton.isVisible({ timeout: 2000 }).catch(() => false)) {
        await threadButton.click();
        await page.waitForTimeout(1000);
        await waitAndScreenshot(page, "thread-sidebar", 500);
        // Close thread sidebar
        const closeThread = page.locator(
          '[data-testid="thread-sidebar"] button[title="Close"], [data-testid="close-thread"]',
        ).first();
        if (await closeThread.isVisible({ timeout: 1000 }).catch(() => false)) {
          await closeThread.click();
          await page.waitForTimeout(500);
        }
      }
    }

    // ===== 11. CHANNEL PINS =====
    const pinsButton = page.locator('button[title*="Pin" i], button[aria-label*="Pin" i]').first();
    if (await pinsButton.isVisible({ timeout: 2000 }).catch(() => false)) {
      await pinsButton.click();
      await page.waitForTimeout(500);
      await waitAndScreenshot(page, "channel-pins", 500);
      // Close pins
      await pinsButton.click();
      await page.waitForTimeout(300);
    }

    // ===== 12. SEARCH =====
    const searchBtn = page.locator('button:has-text("Search"), button[title*="Search" i]').first();
    if (await searchBtn.isVisible({ timeout: 2000 }).catch(() => false)) {
      await searchBtn.click();
      await page.waitForTimeout(500);
    } else {
      await page.keyboard.press("Control+Shift+f");
      await page.waitForTimeout(500);
    }
    const searchInput = page.locator(
      'input[placeholder*="search" i], input[type="search"]',
    ).first();
    if (await searchInput.isVisible({ timeout: 3000 }).catch(() => false)) {
      await searchInput.fill("tournament");
      await page.waitForTimeout(1500);
      await waitAndScreenshot(page, "search-results", 500);
    }

    // ===== 13. VOICE CHANNEL =====
    // Navigate away and back to clear any overlays (search panel)
    await page.goto("https://localhost:5173/", { waitUntil: "domcontentloaded" });
    await expect(page.locator("aside").first()).toBeVisible({ timeout: 15000 });
    await page.waitForTimeout(500);
    if (await pixelLegendsBtn.isVisible({ timeout: 3000 }).catch(() => false)) {
      await pixelLegendsBtn.click();
      await page.waitForTimeout(1000);
    }
    const voiceChannel = page
      .locator("aside")
      .nth(1)
      .locator('[role="button"]')
      .filter({ hasText: /Voice Lounge|voice/i })
      .first();
    if (await voiceChannel.isVisible({ timeout: 3000 }).catch(() => false)) {
      await voiceChannel.click();
      await page.waitForTimeout(1500);
      await waitAndScreenshot(page, "voice-channel", 1000);
    }

    // ===== 14. GUILD SETTINGS (server settings modal) =====
    const guildSettingsBtn = page.locator(
      '[data-testid="guild-settings-button"], button[title*="Server Settings" i]',
    ).first();
    if (await guildSettingsBtn.isVisible({ timeout: 3000 }).catch(() => false)) {
      await guildSettingsBtn.click();
      await page.waitForTimeout(1000);
      await waitAndScreenshot(page, "guild-settings", 500);

      // Take screenshot of roles tab if available
      const rolesTab = page.getByRole("button", { name: /Roles/i }).first();
      if (await rolesTab.isVisible({ timeout: 2000 }).catch(() => false)) {
        await rolesTab.click();
        await page.waitForTimeout(500);
        await waitAndScreenshot(page, "guild-settings-roles", 500);
      }

      // Members tab
      const membersTab = page.getByRole("button", { name: /Members/i }).first();
      if (await membersTab.isVisible({ timeout: 1000 }).catch(() => false)) {
        await membersTab.click();
        await page.waitForTimeout(500);
        await waitAndScreenshot(page, "guild-settings-members", 500);
      }

      // Close guild settings
      const closeModal = page.locator(
        'button[title="Close"], button[aria-label="Close"]',
      ).first();
      if (await closeModal.isVisible({ timeout: 1000 }).catch(() => false)) {
        await closeModal.click();
        await page.waitForTimeout(500);
      } else {
        await page.keyboard.press("Escape");
        await page.waitForTimeout(500);
      }
    }

    // ===== 15. USER SETTINGS =====
    const userSettingsBtn = page.locator(
      '[data-testid="user-settings-button"], button[title="User Settings"]',
    ).first();
    if (await userSettingsBtn.isVisible({ timeout: 3000 }).catch(() => false)) {
      await userSettingsBtn.click();
      await page.waitForTimeout(1000);
      await waitAndScreenshot(page, "user-settings-account", 500);

      // Appearance tab
      const appearanceTab = page.getByRole("button", { name: /Appearance/i }).first();
      if (await appearanceTab.isVisible({ timeout: 1000 }).catch(() => false)) {
        await appearanceTab.click();
        await page.waitForTimeout(500);
        await waitAndScreenshot(page, "user-settings-appearance", 500);
      }

      // Audio tab
      const audioTab = page.getByRole("button", { name: /Audio/i }).first();
      if (await audioTab.isVisible({ timeout: 1000 }).catch(() => false)) {
        await audioTab.click();
        await page.waitForTimeout(500);
        await waitAndScreenshot(page, "user-settings-audio", 500);
      }

      // Security tab
      const securityTab = page.getByRole("button", { name: /Security/i }).first();
      if (await securityTab.isVisible({ timeout: 1000 }).catch(() => false)) {
        await securityTab.click();
        await page.waitForTimeout(500);
        await waitAndScreenshot(page, "user-settings-security", 500);
      }

      // Privacy tab
      const privacyTab = page.getByRole("button", { name: /Privacy/i }).first();
      if (await privacyTab.isVisible({ timeout: 1000 }).catch(() => false)) {
        await privacyTab.click();
        await page.waitForTimeout(500);
        await waitAndScreenshot(page, "user-settings-privacy", 500);
      }

      // Close settings
      const closeSettings = page.locator(
        'button[title="Close"], button[aria-label="Close"]',
      ).first();
      if (await closeSettings.isVisible({ timeout: 1000 }).catch(() => false)) {
        await closeSettings.click();
      } else {
        await page.keyboard.press("Escape");
      }
      await page.waitForTimeout(500);
    }

    // ===== 16. HOME — FRIENDS =====
    const homeBtn = page.locator('button[title="Home"]').first();
    if (await homeBtn.isVisible({ timeout: 2000 }).catch(() => false)) {
      await homeBtn.click();
      await page.waitForTimeout(1000);
      await waitAndScreenshot(page, "home-friends", 500);
    }

    // ===== 17. DM CONVERSATION =====
    const newDmBtn = page.locator(
      '[data-testid="new-dm-button"], button[title*="DM" i], button[title*="Direct" i], button[title*="Message" i]',
    ).first();
    if (await newDmBtn.isVisible({ timeout: 3000 }).catch(() => false)) {
      await newDmBtn.click();
      await page.waitForTimeout(500);
      await waitAndScreenshot(page, "new-dm-dialog", 500);
    }

    // Navigate away to clear any overlays/modals
    await page.goto("https://localhost:5173/", { waitUntil: "domcontentloaded" });
    await expect(page.locator("aside").first()).toBeVisible({ timeout: 15000 });
    await page.waitForTimeout(500);

    // ===== 18. SERVER DISCOVERY =====
    const exploreBtn = page.locator('button[title="Explore Servers"]').first();
    if (await exploreBtn.isVisible({ timeout: 2000 }).catch(() => false)) {
      await exploreBtn.click();
      await page.waitForTimeout(1000);
      await waitAndScreenshot(page, "server-discovery", 500);
    }

    // Navigate away to clear
    await page.goto("https://localhost:5173/", { waitUntil: "domcontentloaded" });
    await expect(page.locator("aside").first()).toBeVisible({ timeout: 15000 });
    await page.waitForTimeout(500);

    // ===== 19. CREATE SERVER DIALOG =====
    const createServerBtn = page.locator('button[title="Create Server"]').first();
    if (await createServerBtn.isVisible({ timeout: 2000 }).catch(() => false)) {
      await createServerBtn.click();
      await page.waitForTimeout(500);
      await waitAndScreenshot(page, "create-server-dialog", 500);
    }

    // Navigate away to clear
    await page.goto("https://localhost:5173/", { waitUntil: "domcontentloaded" });
    await expect(page.locator("aside").first()).toBeVisible({ timeout: 15000 });
    await page.waitForTimeout(500);

    // ===== 20. JOIN SERVER DIALOG =====
    const joinServerBtn = page.locator('button[title="Join Server"]').first();
    if (await joinServerBtn.isVisible({ timeout: 2000 }).catch(() => false)) {
      await joinServerBtn.click();
      await page.waitForTimeout(500);
      await waitAndScreenshot(page, "join-server-dialog", 500);
    }

    // Navigate away to clear
    await page.goto("https://localhost:5173/", { waitUntil: "domcontentloaded" });
    await expect(page.locator("aside").first()).toBeVisible({ timeout: 15000 });
    await page.waitForTimeout(500);

    // ===== 21. KEYBOARD SHORTCUTS =====
    await page.keyboard.press("Control+/");
    await page.waitForTimeout(800);
    const shortcutsDialog = page.locator('[role="dialog"]').first();
    if (await shortcutsDialog.isVisible({ timeout: 2000 }).catch(() => false)) {
      await waitAndScreenshot(page, "keyboard-shortcuts", 500);
    }

    // Navigate away to clear
    await page.goto("https://localhost:5173/", { waitUntil: "domcontentloaded" });
    await expect(page.locator("aside").first()).toBeVisible({ timeout: 15000 });
    await page.waitForTimeout(500);

    // ===== 22. STATUS PICKER =====
    const statusPicker = page.locator('[data-testid="status-picker"]').first();
    if (await statusPicker.isVisible({ timeout: 2000 }).catch(() => false)) {
      await statusPicker.click();
      await page.waitForTimeout(500);
      await waitAndScreenshot(page, "status-picker", 500);
    }

    // Navigate away to clear
    await page.goto("https://localhost:5173/", { waitUntil: "domcontentloaded" });
    await expect(page.locator("aside").first()).toBeVisible({ timeout: 15000 });
    await page.waitForTimeout(500);

    // ===== 23. SECOND GUILD (Code Forge) =====
    const codeForgeBtn = page.locator('button[title="Code Forge"]').first();
    if (await codeForgeBtn.isVisible({ timeout: 3000 }).catch(() => false)) {
      await codeForgeBtn.click();
      await page.waitForTimeout(1000);
      await waitAndScreenshot(page, "second-guild-overview", 500);
    }

    // ===== 24. ADMIN DASHBOARD =====
    await page.goto("https://localhost:5173/admin", {
      waitUntil: "domcontentloaded",
    });
    await page.waitForTimeout(2000);
    const adminHeading = page.getByRole("heading", { name: "Admin Dashboard" });
    if (await adminHeading.isVisible({ timeout: 5000 }).catch(() => false)) {
      await waitAndScreenshot(page, "admin-dashboard", 500);

      // Users panel
      const usersPanel = page.getByRole("button", { name: /Users/i }).first();
      if (await usersPanel.isVisible({ timeout: 1000 }).catch(() => false)) {
        await usersPanel.click();
        await page.waitForTimeout(500);
        await waitAndScreenshot(page, "admin-users", 500);
      }

      // Guilds panel
      const guildsPanel = page.getByRole("button", { name: /Guilds|Servers/i }).first();
      if (await guildsPanel.isVisible({ timeout: 1000 }).catch(() => false)) {
        await guildsPanel.click();
        await page.waitForTimeout(500);
        await waitAndScreenshot(page, "admin-guilds", 500);
      }

      // Audit log
      const auditPanel = page.getByRole("button", { name: /Audit/i }).first();
      if (await auditPanel.isVisible({ timeout: 1000 }).catch(() => false)) {
        await auditPanel.click();
        await page.waitForTimeout(500);
        await waitAndScreenshot(page, "admin-audit-log", 500);
      }

      // Command Center
      const commandCenter = page.getByRole("button", { name: /Command/i }).first();
      if (await commandCenter.isVisible({ timeout: 1000 }).catch(() => false)) {
        await commandCenter.click();
        await page.waitForTimeout(500);
        await waitAndScreenshot(page, "admin-command-center", 500);
      }
    }

    // ===== 25. COMMAND PALETTE =====
    await page.goto("https://localhost:5173/", { waitUntil: "domcontentloaded" });
    await expect(page.locator("aside").first()).toBeVisible({ timeout: 15000 });
    await page.waitForTimeout(1000);

    await page.keyboard.press("Control+k");
    await page.waitForTimeout(500);
    const palette = page.locator('[role="dialog"]').first();
    if (await palette.isVisible({ timeout: 2000 }).catch(() => false)) {
      await waitAndScreenshot(page, "command-palette", 500);
    }

    // ===== FINAL: FULL APP OVERVIEW =====
    await page.goto("https://localhost:5173/", { waitUntil: "domcontentloaded" });
    await expect(page.locator("aside").first()).toBeVisible({ timeout: 15000 });
    await page.waitForTimeout(500);
    if (await pixelLegendsBtn.isVisible({ timeout: 3000 }).catch(() => false)) {
      await pixelLegendsBtn.click();
      await page.waitForTimeout(500);
    }
    if (await generalChannel.isVisible({ timeout: 3000 }).catch(() => false)) {
      await generalChannel.click();
      await page.waitForTimeout(1500);
    }
    await waitAndScreenshot(page, "full-app-overview", 500);

    await context.close();
  });
});
