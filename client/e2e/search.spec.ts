import { test, expect } from "@playwright/test";
import {
  registerAndReachMain,
  uniqueId,
  ensureGuildSelected,
  createTextChannel,
  selectChannel,
  sendMessage,
  openSearch,
} from "./helpers";

test.describe("Search", () => {
  test("search panel opens via sidebar button", async ({ page }) => {
    await registerAndReachMain(page);
    await ensureGuildSelected(page);

    await openSearch(page);

    await expect(page.getByTestId("search-input")).toBeVisible({
      timeout: 5000,
    });
  });

  test("search finds a sent message", async ({ page }) => {
    await registerAndReachMain(page);
    await ensureGuildSelected(page);
    const channelName = await createTextChannel(page);
    await selectChannel(page, channelName);

    const searchTerm = `findme-${uniqueId("search")}`;
    await sendMessage(page, searchTerm);

    await openSearch(page);

    const searchInput = page.getByTestId("search-input");
    await expect(searchInput).toBeVisible({ timeout: 5000 });
    await searchInput.fill(searchTerm);

    // Wait for search results
    await expect(
      page.getByTestId("search-results").getByText(searchTerm),
    ).toBeVisible({ timeout: 15000 });
  });

  test("search with no results shows empty state", async ({ page }) => {
    await registerAndReachMain(page);
    await ensureGuildSelected(page);

    await openSearch(page);

    const searchInput = page.getByTestId("search-input");
    await expect(searchInput).toBeVisible({ timeout: 5000 });
    await searchInput.fill("zzz_nonexistent_query_xyz_99999");

    await expect(page.getByText(/no results/i)).toBeVisible({
      timeout: 10000,
    });
  });
});
