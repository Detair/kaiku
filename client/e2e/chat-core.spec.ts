import { test, expect } from "@playwright/test";
import { registerAndReachMain, uniqueId } from "./helpers";

test.describe("Chat core", () => {
  test("sends a real message in the default guild channel", async ({ page }) => {
    await registerAndReachMain(page, {
      usernamePrefix: "chat",
      setupServerName: "E2E Chat Server",
    });

    const guildButtons = page.getByTestId("guild-button");
    if ((await guildButtons.count()) === 0) {
      const guildName = uniqueId("ChatGuild");
      await page.getByTestId("create-server-button").click();
      const createGuildName = page.getByTestId("create-guild-name");
      await expect(createGuildName).toBeVisible({ timeout: 10000 });
      await createGuildName.fill(guildName);
      await page.getByTestId("create-guild-submit").click();
      await expect(guildButtons.first()).toBeVisible({ timeout: 15000 });
    }

    await guildButtons.first().click();

    const channelName = uniqueId("chat-core");
    await page.getByTestId("create-channel-button").first().click();
    const createChannelName = page.getByTestId("create-channel-name");
    await expect(createChannelName).toBeVisible({ timeout: 10000 });
    await createChannelName.fill(channelName);
    await page.getByTestId("create-channel-submit").click();
    await page.getByText(channelName).first().click();

    const input = page.getByTestId("message-input");
    await expect(input).toBeVisible({ timeout: 10000 });

    const message = `chat-core-${uniqueId("msg")}`;
    await input.fill(message);
    await input.press("Enter");

    await expect(page.getByText(message)).toBeVisible({ timeout: 15000 });
  });
});
