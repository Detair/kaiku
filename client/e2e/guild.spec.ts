import { test, expect } from "@playwright/test";
import {
  registerAndReachMain,
  createGuild,
  ensureGuildSelected,
  openGuildSettings,
} from "./helpers";

test.describe("Guild Management", () => {
  test("create guild appears in server rail", async ({ page }) => {
    await registerAndReachMain(page);
    const guildName = await createGuild(page);
    await expect(
      page.getByTestId("guild-button").filter({ hasText: guildName }),
    ).toBeVisible({ timeout: 15000 });
  });

  test("guild settings modal opens with tabs", async ({ page }) => {
    await registerAndReachMain(page);
    await ensureGuildSelected(page);
    await openGuildSettings(page);

    // As the first user (owner), all tabs should be visible
    await expect(page.getByTestId("guild-tab-general")).toBeVisible({
      timeout: 5000,
    });
    await expect(page.getByTestId("guild-tab-invites")).toBeVisible();
    await expect(page.getByTestId("guild-tab-members")).toBeVisible();
    await expect(page.getByTestId("guild-tab-roles")).toBeVisible();
  });

  test("create invite shows invite code", async ({ page }) => {
    await registerAndReachMain(page);
    await ensureGuildSelected(page);
    await openGuildSettings(page);

    await page.getByTestId("guild-tab-invites").click();
    await page.getByTestId("create-invite-button").click();
    await expect(page.getByTestId("invite-code")).toBeVisible({
      timeout: 10000,
    });
  });

  test("member list displays current user", async ({ page }) => {
    const { username } = await registerAndReachMain(page);
    await ensureGuildSelected(page);
    await openGuildSettings(page);

    await page.getByTestId("guild-tab-members").click();
    await expect(page.getByText(username)).toBeVisible({ timeout: 10000 });
  });
});
