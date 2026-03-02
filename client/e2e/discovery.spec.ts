import { test, expect } from "@playwright/test";
import { registerAndReachMain } from "./helpers";

test.describe("Server Discovery", () => {
  test("discovery view loads via create server modal", async ({ page }) => {
    await registerAndReachMain(page);

    // Navigate to discovery via the Join Server flow
    await page.getByTestId("create-server-button").click();

    // Look for a discover/browse option in the modal
    const browseOption = page.getByText(/browse|discover|explore/i).first();
    await expect(browseOption).toBeVisible({ timeout: 5000 });
    await browseOption.click();
    await expect(page.getByTestId("discovery-search")).toBeVisible({
      timeout: 10000,
    });
  });
});
