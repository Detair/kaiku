#!/usr/bin/env node
// Opt-in passkey check in a real Chromium with a CDP virtual authenticator
// (LP, review round 1). Not part of CI: it needs a running server, the web
// dev server and a throwaway database, and it writes accounts and passkeys.
//
//   KAIKU_PASSKEY_BROWSER=1 node scripts/passkey-browser-check.mjs
//
// `scripts/passkey-browser-check.sh` (repo root) starts the server and the
// web dev server for it. Steps:
//   1. web sign-in with a passkey (seeded fixture, as `seed_passkey` in the
//      server tests: the product path to a first passkey is a key phone);
//   2. web register: Settings -> Security -> "Add a passkey" (step-up with
//      the first passkey, then a new credential in the browser);
//   3. web sign-in with only the new passkey;
//   4. desktop sign-in: the ceremony page in the browser, the auto-submitted
//      hand-off to a test loopback listener, redemption with the verifier;
//   5. desktop step-up (passkey.remove) the same way.
//
// Environment:
//   KAIKU_PASSKEY_BROWSER=1    required, otherwise the check is skipped
//   KAIKU_PASSKEY_WEB_URL      web origin = PUBLIC_URL (default https://localhost:5173)
//   KAIKU_PASSKEY_SQL          shell command that runs SQL from stdin against the
//                              server's database (default: psql "$DATABASE_URL")
//   KAIKU_PASSKEY_CHROME       Chromium executable (default: Playwright's)
//   KAIKU_PASSKEY_HEADED=1     show the browser
//   KAIKU_PASSKEY_SCREENSHOT   file for a screenshot when a step fails

import { execSync } from "node:child_process";
import { createHash, generateKeyPairSync, randomBytes, randomUUID } from "node:crypto";
import { createServer } from "node:http";
import { chromium, request } from "@playwright/test";

if (process.env.KAIKU_PASSKEY_BROWSER !== "1") {
  console.log("passkey browser check skipped (set KAIKU_PASSKEY_BROWSER=1)");
  process.exit(0);
}

const WEB = (process.env.KAIKU_PASSKEY_WEB_URL ?? "https://localhost:5173").replace(/\/+$/, "");
const RP_ID = new URL(WEB).hostname;
const SQL = process.env.KAIKU_PASSKEY_SQL ?? 'psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -q';
const DESKTOP = { "X-Kaiku-Client": "desktop" };

const b64u = (buf) => Buffer.from(buf).toString("base64url");
const b64 = (buf) => Buffer.from(buf).toString("base64");
const uuidBytes = (uuid) => Buffer.from(uuid.replace(/-/g, ""), "hex");

let step = "setup";
function check(cond, message) {
  if (!cond) throw new Error(`${step}: ${message}`);
}
function sql(text) {
  execSync(SQL, { input: text, stdio: ["pipe", "ignore", "inherit"], shell: "/bin/sh" });
}
const quote = (s) => `'${String(s).replace(/'/g, "''")}'`;

/** A P-256 passkey in webauthn-rs's stored form plus the CDP credential. */
function fixturePasskey(userHandle) {
  const { privateKey, publicKey } = generateKeyPairSync("ec", { namedCurve: "P-256" });
  const jwk = publicKey.export({ format: "jwk" });
  const credId = randomBytes(16);
  const stored = {
    cred: {
      cred_id: b64u(credId),
      cred: { type_: "ES256", key: { EC_EC2: { curve: "SECP256R1", x: jwk.x, y: jwk.y } } },
      counter: 0,
      transports: null,
      user_verified: true,
      backup_eligible: false,
      backup_state: false,
      registration_policy: "required",
      extensions: {
        cred_protect: "NotRequested",
        hmac_create_secret: "NotRequested",
        appid: "NotRequested",
        cred_props: "NotRequested",
      },
      attestation: { data: "None", metadata: "None" },
      attestation_format: "none",
    },
  };
  const cdp = {
    credentialId: b64(credId),
    isResidentCredential: true,
    rpId: RP_ID,
    privateKey: b64(privateKey.export({ format: "der", type: "pkcs8" })),
    userHandle: b64(uuidBytes(userHandle)),
    signCount: 0,
  };
  return { credId, stored, cdp };
}

/** A one-shot loopback listener like the desktop app's (C29 step 4). */
function loopback() {
  const callbackPath = b64u(randomBytes(32));
  let resolve;
  const received = new Promise((r) => (resolve = r));
  const server = createServer((req, res) => {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      const ok =
        req.method === "POST" &&
        req.url === `/callback/${callbackPath}` &&
        req.headers.host === `127.0.0.1:${server.address().port}` &&
        (req.headers["content-type"] ?? "").startsWith("application/x-www-form-urlencoded");
      res.writeHead(ok ? 200 : 404, { "Content-Type": "text/html; charset=utf-8" });
      res.end(ok ? "<p>You can close this page.</p>" : "not found");
      if (ok) resolve(Object.fromEntries(new URLSearchParams(body)));
    });
  });
  return new Promise((r) =>
    server.listen(0, "127.0.0.1", () => r({ server, port: server.address().port, callbackPath, received })),
  );
}

async function completeOnboardingIfVisible(page) {
  const dialog = page.getByTestId("onboarding-wizard");
  if (!(await dialog.waitFor({ state: "visible", timeout: 5000 }).then(() => true, () => false))) return;
  const next = dialog.getByRole("button", { name: /^(Get Started|Continue|Next|Skip|Finish|Done)/ });
  for (let i = 0; i < 10 && (await dialog.isVisible()); i += 1) {
    await next.last().click({ timeout: 5000 }).catch(() => {});
    await page.waitForTimeout(300);
  }
  check(!(await dialog.isVisible()), "onboarding wizard closed");
}

async function waitForApp(page) {
  await page.locator("aside").first().waitFor({ state: "visible", timeout: 20_000 });
  await completeOnboardingIfVisible(page);
}

/**
 * Button sign-in: the login page's conditional (autofill) request starts
 * while the authenticator holds no credential, so it stays pending; the
 * credential comes back, and the button must abort that request and run
 * its own.
 */
async function signInWithButton(page, cdp, authenticatorId) {
  const { credentials } = await cdp.send("WebAuthn.getCredentials", { authenticatorId });
  await cdp.send("WebAuthn.clearCredentials", { authenticatorId });
  await page.goto(`${WEB}/login`);
  const button = page.getByRole("button", { name: "Sign in with passkey" });
  await button.waitFor({ state: "visible" });
  await page.waitForTimeout(500);
  for (const credential of credentials) {
    await cdp.send("WebAuthn.addCredential", { authenticatorId, credential });
  }
  await button.click();
  await waitForApp(page);
}

async function signOut(context, page) {
  await context.clearCookies();
  await page.evaluate(() => localStorage.clear());
}

/** Desktop flow: start, ceremony page in the browser, hand-off, redeem. */
async function desktopFlow(api, page, body, bearer) {
  const verifier = b64u(randomBytes(32));
  const challenge = b64u(createHash("sha256").update(verifier).digest());
  const lb = await loopback();
  try {
    const started = await api.post("/auth/passkeys/desktop/flows", {
      headers: { ...DESKTOP, ...(bearer ? { Authorization: `Bearer ${bearer}` } : {}) },
      data: { ...body, loopback_port: lb.port, callback_path: lb.callbackPath, client_challenge: challenge },
    });
    check(started.status() === 201, `flow start HTTP ${started.status()} ${await started.text()}`);
    const flow = await started.json();
    check(flow.url.startsWith(`${WEB}/auth/passkeys/desktop/`), "flow URL is on PUBLIC_URL");
    await page.goto(flow.url);
    const heading = (await page.locator("h1").textContent()) ?? "";
    await page.locator("#start").click();
    const form = await Promise.race([
      lb.received,
      new Promise((_, reject) => setTimeout(() => reject(new Error(`${step}: no hand-off POST`)), 20_000)),
    ]);
    check(form.flow_id === flow.flow_id, "hand-off names the flow");
    check(typeof form.code === "string" && !form.error, `hand-off error ${form.error}`);
    // A step-up is redeemed by the session that started it.
    const redeemed = await api.post("/auth/passkeys/desktop/redeem", {
      headers: { ...DESKTOP, ...(bearer ? { Authorization: `Bearer ${bearer}` } : {}) },
      data: { flow_id: flow.flow_id, code: form.code, verifier },
    });
    check(redeemed.status() === 200, `redeem HTTP ${redeemed.status()} ${await redeemed.text()}`);
    return { heading, result: await redeemed.json() };
  } finally {
    lb.server.close();
  }
}

const api = await request.newContext({ baseURL: WEB, ignoreHTTPSErrors: true });
let page;
const browser = await chromium.launch({
  executablePath: process.env.KAIKU_PASSKEY_CHROME || undefined,
  headless: process.env.KAIKU_PASSKEY_HEADED !== "1",
});
try {
  // Accounts: a first one (on a fresh database it becomes the admin), then
  // the test user.
  const tag = randomBytes(4).toString("hex");
  const password = b64u(randomBytes(18));
  for (const username of [`pkadmin_${tag}`, `pkuser_${tag}`]) {
    const r = await api.post("/auth/register", {
      data: { username, password, display_name: username, email: `${username}@example.com` },
    });
    check(r.ok(), `register ${username} HTTP ${r.status()} ${await r.text()}`);
  }
  const username = `pkuser_${tag}`;
  const handle = randomUUID();
  const seeded = fixturePasskey(handle);
  // Fixtures on the throwaway database: setup done (no wizard), the passkey.
  sql(
    `UPDATE server_config SET value = 'true'::jsonb WHERE key = 'setup_complete';
     UPDATE users SET webauthn_user_handle = ${quote(handle)} WHERE username = ${quote(username)};
     INSERT INTO passkeys (id, user_id, credential_id, credential, label, backup_eligible, backup_state, state)
     SELECT gen_random_uuid(), id, decode(${quote(seeded.credId.toString("hex"))}, 'hex'),
            ${quote(JSON.stringify(seeded.stored))}::jsonb, 'Seeded passkey', false, false, 'active'
       FROM users WHERE username = ${quote(username)};`,
  );

  const context = await browser.newContext({ ignoreHTTPSErrors: true, locale: "en-US" });
  page = await context.newPage();
  const cdp = await context.newCDPSession(page);
  await cdp.send("WebAuthn.enable", { enableUI: false });
  const { authenticatorId } = await cdp.send("WebAuthn.addVirtualAuthenticator", {
    options: {
      protocol: "ctap2",
      ctap2Version: "ctap2_1",
      transport: "internal",
      hasResidentKey: true,
      hasUserVerification: true,
      isUserVerified: true,
      automaticPresenceSimulation: true,
    },
  });
  await cdp.send("WebAuthn.addCredential", { authenticatorId, credential: seeded.cdp });

  step = "web sign-in, autofill (seeded passkey)";
  // The virtual authenticator answers the login page's conditional request
  // (automatic presence), as choosing the passkey in the autofill would.
  await page.goto(`${WEB}/login`);
  await waitForApp(page);
  console.log(`ok  ${step}`);

  step = "web register refused (authenticator already holds the account's passkey)";
  await page.click('button[title="User Settings"]');
  await page.getByRole("dialog").getByText("Security", { exact: true }).first().click();
  const add = page.getByRole("button", { name: "Add a passkey" });
  await add.click();
  // excludeCredentials: the step-up succeeds, the new credential is refused.
  await page.getByText("This passkey is already added.").waitFor({ state: "visible", timeout: 20_000 });
  console.log(`ok  ${step}`);

  step = "web register";
  // The first passkey stands for the key phone's: it leaves this
  // authenticator once the step-up has used it.
  let moved = false;
  cdp.on("WebAuthn.credentialAsserted", (e) => {
    if (!moved && e.credential.credentialId === seeded.cdp.credentialId) {
      moved = true;
      void cdp.send("WebAuthn.removeCredential", { authenticatorId, credentialId: seeded.cdp.credentialId });
    }
  });
  await add.click();
  const list = page.locator('section[aria-labelledby="passkeys-title"] li');
  await list.nth(1).waitFor({ state: "visible", timeout: 20_000 });
  check(moved, "the step-up used the first passkey");
  const creds = (await cdp.send("WebAuthn.getCredentials", { authenticatorId })).credentials;
  check(creds.length === 1, `authenticator holds ${creds.length} credentials`);
  const created = creds[0];
  check(created.credentialId !== seeded.cdp.credentialId, "a new credential");
  check(created.isResidentCredential, "the new passkey is discoverable");
  check(created.userHandle === seeded.cdp.userHandle, "the new passkey has the account's user handle");
  console.log(`ok  ${step}`);

  step = "web sign-in, button (browser-registered passkey only)";
  await signOut(context, page);
  await signInWithButton(page, cdp, authenticatorId);
  console.log(`ok  ${step}`);

  step = "desktop sign-in";
  const signIn = await desktopFlow(api, page, { purpose: "sign_in", username: username.toUpperCase() });
  check(signIn.result.purpose === "sign_in", `purpose ${signIn.result.purpose}`);
  check(typeof signIn.result.access_token === "string", "access token");
  const me = await api.get("/auth/me", {
    headers: { ...DESKTOP, Authorization: `Bearer ${signIn.result.access_token}` },
  });
  check(me.ok() && (await me.json()).username === username, "the session is the test user's");
  console.log(`ok  ${step}`);

  step = "desktop step-up";
  const listed = await api.get("/auth/passkeys", {
    headers: { ...DESKTOP, Authorization: `Bearer ${signIn.result.access_token}` },
  });
  check(listed.ok(), `list HTTP ${listed.status()}`);
  const seededRow = (await listed.json()).passkeys.find((p) => p.label === "Seeded passkey");
  check(seededRow, "seeded passkey listed");
  const stepUp = await desktopFlow(
    api,
    page,
    { purpose: "step_up", action: "passkey.remove", target: seededRow.id },
    signIn.result.access_token,
  );
  check(stepUp.heading.includes("Seeded passkey"), `heading names the passkey: ${stepUp.heading}`);
  check(stepUp.result.purpose === "step_up", `purpose ${stepUp.result.purpose}`);
  check(stepUp.result.grant?.action === "passkey.remove", "grant action");
  console.log(`ok  ${step}`);

  console.log("passkey browser check passed");
} catch (err) {
  console.error(`FAILED ${err.message}`);
  process.exitCode = 1;
  if (page && process.env.KAIKU_PASSKEY_SCREENSHOT) {
    console.error(`at ${page.url()}`);
    await page.screenshot({ path: process.env.KAIKU_PASSKEY_SCREENSHOT }).catch(() => {});
  }
} finally {
  await browser.close();
  await api.dispose();
}
