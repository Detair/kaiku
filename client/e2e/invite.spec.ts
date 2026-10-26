import { test, expect } from "@playwright/test";
import {
  registerAndReachMain,
  ensureGuildSelected,
  openGuildSettings,
} from "./helpers";

test.describe("Invite Links", () => {
  test("create invite and get code", async ({ page }) => {
    await registerAndReachMain(page);
    await ensureGuildSelected(page);
    await openGuildSettings(page);

    await page.getByTestId("guild-tab-invites").click();
    await page.getByTestId("create-invite-button").click();

    const inviteCode = page.getByTestId("invite-code");
    await expect(inviteCode).toBeVisible({ timeout: 10000 });

    // The list shows the bare code, not a link
    const codeText = await inviteCode.textContent();
    expect(codeText?.trim()).toMatch(/^[a-zA-Z0-9]+$/);
  });

  test("join guild via invite route", async ({ page, browser }) => {
    // First user creates a guild and invite
    await registerAndReachMain(page);
    const guildName = await ensureGuildSelected(page);
    await openGuildSettings(page);

    await page.getByTestId("guild-tab-invites").click();
    await page.getByTestId("create-invite-button").click();

    const inviteCode = page.getByTestId("invite-code");
    await expect(inviteCode).toBeVisible({ timeout: 10000 });

    const code = (await inviteCode.textContent())?.trim();
    expect(code).toMatch(/^[a-zA-Z0-9]+$/);

    // Second user joins via invite
    const context2 = await browser.newContext({ ignoreHTTPSErrors: true });
    const page2 = await context2.newPage();
    await registerAndReachMain(page2, { usernamePrefix: "invitee" });

    await page2.goto(`/invite/${code}`);
    // Wait for the specific invited guild to appear in the server rail
    await expect(
      page2.getByTestId("guild-button").filter({ hasText: guildName }),
    ).toBeVisible({ timeout: 15000 });

    await context2.close();
  });

  test("invalid invite shows error", async ({ page }) => {
    await registerAndReachMain(page);
    await page.goto("/invite/invalidcode99999");

    // Should show an explicit error message for invalid invites
    await expect(
      page.getByText(/invalid|expired|not found/i).first(),
    ).toBeVisible({ timeout: 10000 });
  });
});
