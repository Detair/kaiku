import { describe, it, expect } from "vitest";
import { androidIntentUrl, isAndroidBrowser, isAppMissingPage, parseSetupFragment } from "../setupLink";

const TOKEN = "QEFCQ0RFRkdISUpLTE1OT1BRUlNUVVZXWFlaW1xdXl8";

describe("setup links", () => {
  it("parses v=1 and the token", () => {
    expect(parseSetupFragment(`#v=1&t=${TOKEN}`)).toEqual({ v: 1, token: TOKEN });
    expect(parseSetupFragment(`v=1&t=${TOKEN}`)).toEqual({ v: 1, token: TOKEN });
  });

  it("an unknown version asks for an update; a missing or bad token is missing", () => {
    expect(parseSetupFragment(`#v=2&t=${TOKEN}`)).toEqual({ error: "version" });
    expect(parseSetupFragment("#v=1")).toEqual({ error: "missing" });
    expect(parseSetupFragment("#v=1&t=not+b64u!")).toEqual({ error: "missing" });
    expect(parseSetupFragment("")).toEqual({ error: "missing" });
  });

  it("builds the Android intent URL with the encoded origin, for the Kaiku app only", () => {
    const fallback = encodeURIComponent("https://chat.example.com/setup?app=missing");
    expect(androidIntentUrl("https://chat.example.com", TOKEN)).toBe(
      `intent://setup/v1?o=${encodeURIComponent("https://chat.example.com")}&t=${TOKEN}#Intent;scheme=kaiku;package=io.wolftown.kaiku;S.browser_fallback_url=${fallback};end`,
    );
  });

  it("sends a phone without the app back to this server's page, never with the setup code", () => {
    const url = androidIntentUrl("https://chat.example.com", TOKEN);
    const extra = url.split("#Intent;")[1].split(";").find((p) => p.startsWith("S.browser_fallback_url="));
    expect(extra).toBeDefined();
    const fallback = decodeURIComponent(extra!.slice("S.browser_fallback_url=".length));
    expect(fallback).toBe("https://chat.example.com/setup?app=missing");
    expect(fallback).not.toContain(TOKEN);
    expect(isAppMissingPage(new URL(fallback).search)).toBe(true);
    expect(isAppMissingPage("")).toBe(false);
    expect(isAppMissingPage("?app=other")).toBe(false);
  });

  it("recognises Android browsers", () => {
    expect(isAndroidBrowser("Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 Chrome/130 Mobile Safari/537.36")).toBe(true);
    expect(isAndroidBrowser("Mozilla/5.0 (X11; Linux x86_64; rv:132.0) Gecko/20100101 Firefox/132.0")).toBe(false);
  });
});
