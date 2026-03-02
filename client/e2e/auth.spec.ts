import { test, expect } from "@playwright/test";
import { registerAndReachMain, uniqueUsername, login } from "./helpers";

test.describe("Authentication", () => {
  test("login form displays required fields", async ({ page }) => {
    await page.goto("/login");

    await expect(page.getByTestId("login-username")).toBeVisible({ timeout: 10000 });
    await expect(page.getByTestId("login-password")).toBeVisible();
    await expect(page.getByTestId("login-submit")).toBeVisible();
  });

  test("successful login after registration", async ({ page }) => {
    const { username } = await registerAndReachMain(page);

    await page.getByTestId("logout-button").click();
    await expect(page).toHaveURL(/\/login/, { timeout: 15000 });

    await login(page, username, "password123");

    await expect(page.getByTestId("user-settings-button")).toBeVisible({ timeout: 15000 });
  });

  test("invalid credentials show error", async ({ page }) => {
    await page.goto("/login");

    const username = uniqueUsername("badcreds");
    await page.getByTestId("login-username").fill(username);
    await page.getByTestId("login-password").fill("wrongpassword99");
    await page.getByTestId("login-submit").click();

    await expect(
      page.getByTestId("login-error").or(page.getByText(/invalid|error|failed|incorrect/i)),
    ).toBeVisible({ timeout: 15000 });
  });

  test("registration creates account and enters app", async ({ page }) => {
    await registerAndReachMain(page);

    await expect(page.getByTestId("user-settings-button")).toBeVisible({ timeout: 15000 });
  });

  test("registration validation rejects mismatched passwords", async ({ page }) => {
    await page.goto("/register");

    await page.getByTestId("register-server-url").fill("http://localhost:8080");
    await page.getByTestId("register-username").fill(uniqueUsername("mismatch"));
    await page.getByTestId("register-password").fill("pass1234");
    await page.getByTestId("register-password-confirm").fill("different");
    await page.getByTestId("register-submit").click();

    await expect(page.getByText("Passwords do not match")).toBeVisible({ timeout: 10000 });
  });

  test("login and register pages have navigation links", async ({ page }) => {
    await page.goto("/login");
    const registerLink = page.getByRole("link", { name: "Register" });
    await expect(registerLink).toBeVisible({ timeout: 10000 });
    await expect(registerLink).toHaveAttribute("href", "/register");

    await page.goto("/register");
    const loginLink = page.getByRole("link", { name: "Login" });
    await expect(loginLink).toBeVisible({ timeout: 10000 });
    await expect(loginLink).toHaveAttribute("href", "/login");
  });

  test("unauthenticated access redirects to login", async ({ page }) => {
    await page.goto("/");
    await expect(page).toHaveURL(/\/(login|register)/, { timeout: 15000 });
  });

  test("logout redirects to login", async ({ page }) => {
    await registerAndReachMain(page);

    await page.getByTestId("logout-button").click();

    await expect(page).toHaveURL(/\/login/, { timeout: 15000 });
  });
});
