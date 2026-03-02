import { test, expect } from "@playwright/test";
import {
  registerAndReachMain,
  createTextChannel,
  selectChannel,
  ensureGuildSelected,
} from "./helpers";

test.describe("Channel Management", () => {
  test("channel list displays after selecting guild", async ({ page }) => {
    await registerAndReachMain(page);
    await ensureGuildSelected(page);
    // Default "general" channel or created channels should appear
    await expect(page.getByTestId("channel-item").first()).toBeVisible({
      timeout: 10000,
    });
  });

  test("create text channel appears in list", async ({ page }) => {
    await registerAndReachMain(page);
    await ensureGuildSelected(page);
    const channelName = await createTextChannel(page);
    await expect(page.getByText(channelName).first()).toBeVisible({
      timeout: 10000,
    });
  });

  test("selecting channel shows message input", async ({ page }) => {
    await registerAndReachMain(page);
    await ensureGuildSelected(page);
    const channelName = await createTextChannel(page);
    await selectChannel(page, channelName);
    await expect(page.getByTestId("message-input")).toBeVisible({
      timeout: 10000,
    });
  });

  test("channel context menu appears on right-click", async ({ page }) => {
    await registerAndReachMain(page);
    await ensureGuildSelected(page);
    const channelName = await createTextChannel(page);

    const channelItem = page.getByText(channelName).first();
    await channelItem.click({ button: "right" });
    await expect(page.getByTestId("context-menu")).toBeVisible({
      timeout: 5000,
    });
  });
});
