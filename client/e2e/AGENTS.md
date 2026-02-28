<!-- Parent: ../AGENTS.md -->
# End-to-End Tests

## Purpose

Playwright end-to-end tests for the Kaiku client. Tests user flows through the actual application UI.

## Key Files

| File | Purpose |
|------|---------|
| `chat.spec.ts` | Chat functionality tests: sending messages, channel navigation |

## For AI Agents

### Running E2E Tests

```bash
cd client

# Install Playwright browsers (first time)
npx playwright install

# Run all E2E tests
bun run test:e2e

# Run specific test file
npx playwright test e2e/chat.spec.ts

# Run with UI mode (debugging)
npx playwright test --ui

# Run headed (see browser)
npx playwright test --headed
```

### Test Structure

```typescript
import { test, expect } from '@playwright/test';

test.describe('Chat', () => {
  test.beforeEach(async ({ page }) => {
    // Login or navigate to starting state
    await page.goto('/');
  });

  test('should send a message', async ({ page }) => {
    await page.fill('[data-testid="message-input"]', 'Hello');
    await page.click('[data-testid="send-button"]');
    await expect(page.locator('.message')).toContainText('Hello');
  });
});
```

### Test Environment

E2E tests require:
1. Running backend server
2. Running frontend dev server
3. Test database with seed data

```bash
# Start all services
make dev

# In another terminal
cd client && bun run test:e2e
```

### Writing New Tests

**Selectors:**
- Prefer `data-testid` attributes
- Fallback to semantic selectors (role, label)
- Avoid CSS class selectors (brittle)

**Assertions:**
- Use Playwright's auto-waiting assertions
- Check for visible elements before interaction
- Verify state changes, not just clicks

### Test Data

Tests should:
- Use seed data from `server/seeds/`
- Clean up created data (or use test isolation)
- Not depend on specific database state

### CI Integration

E2E tests run in GitHub Actions:
- Uses Playwright's container images
- Screenshots on failure saved as artifacts
- Runs after unit tests pass

### Configuration

See `playwright.config.ts` in client root:
- Base URL configuration
- Browser selection (Chromium, Firefox, WebKit)
- Screenshot and video settings
- Timeouts and retries

## Dependencies

- Playwright test framework
- Running application stack
- Node.js (required for Playwright, even with Bun)
