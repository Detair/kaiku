import { test, expect } from "@playwright/test";
import {
  registerAndReachMain,
  uniqueId,
  createGuild,
  createTextChannel,
  selectChannel,
  goHome,
} from "./helpers";

test.describe("App Navigation", () => {
  test("app shell layout visible after login", async ({ page }) => {
    await registerAndReachMain(page);

    await expect(page.getByTestId("server-rail")).toBeVisible({ timeout: 10000 });
    await expect(page.getByTestId("user-settings-button")).toBeVisible({ timeout: 10000 });
  });

  test("home button navigates to home view", async ({ page }) => {
    await registerAndReachMain(page);
    await createGuild(page);
    await goHome(page);

    await expect(page.getByText("Friends")).toBeVisible({ timeout: 10000 });
  });

  test("guild switching updates sidebar", async ({ page }) => {
    await registerAndReachMain(page);
    const guild1 = await createGuild(page, uniqueId("NavGuild1"));
    const guild2 = await createGuild(page, uniqueId("NavGuild2"));

    await page.getByTestId("guild-button").filter({ hasText: guild1 }).click();
    await expect(page.getByText(guild1)).toBeVisible({ timeout: 10000 });

    await page.getByTestId("guild-button").filter({ hasText: guild2 }).click();
    await expect(page.getByText(guild2)).toBeVisible({ timeout: 10000 });
  });

  test("channel selection shows message input", async ({ page }) => {
    await registerAndReachMain(page);
    await createGuild(page);

    const guildButton = page.getByTestId("guild-button").first();
    await expect(guildButton).toBeVisible({ timeout: 10000 });
    await guildButton.click();

    const channelName = await createTextChannel(page);
    await selectChannel(page, channelName);

    await expect(page.getByTestId("message-input")).toBeVisible({ timeout: 10000 });
  });

  test("user panel has settings and logout buttons", async ({ page }) => {
    await registerAndReachMain(page);

    await expect(page.getByTestId("user-settings-button")).toBeVisible({ timeout: 10000 });
    await expect(page.getByTestId("logout-button")).toBeVisible({ timeout: 10000 });
  });

  test("command palette opens and closes", async ({ page }) => {
    await registerAndReachMain(page);

    await page.keyboard.press("Control+k");
    await expect(page.getByTestId("command-palette")).toBeVisible({ timeout: 5000 });
    await expect(page.getByTestId("command-palette-input")).toBeVisible({ timeout: 5000 });

    await page.keyboard.press("Escape");
    await expect(page.getByTestId("command-palette")).toBeHidden({ timeout: 5000 });
  });
});
