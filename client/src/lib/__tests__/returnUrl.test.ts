import { describe, expect, it } from "vitest";
import { safeReturnUrl } from "../returnUrl";

describe("safeReturnUrl", () => {
  it("accepts a same-origin path", () => {
    expect(safeReturnUrl("%2Finvite%2Fabc123")).toBe("/invite/abc123");
    expect(safeReturnUrl(["/channels/1", "/other"])).toBe("/channels/1");
  });

  it("rejects missing, external and protocol-relative values", () => {
    expect(safeReturnUrl(undefined)).toBeNull();
    expect(safeReturnUrl("")).toBeNull();
    expect(safeReturnUrl("https://evil.example")).toBeNull();
    expect(safeReturnUrl("//evil.example")).toBeNull();
  });

  it("rejects backslash forms that browsers treat as protocol-relative", () => {
    expect(safeReturnUrl("/\\evil.example")).toBeNull();
    expect(safeReturnUrl("%2F%5Cevil.example")).toBeNull();
  });

  it("rejects control characters that browsers strip into //host", () => {
    expect(safeReturnUrl("/%09/evil.example")).toBeNull();
    expect(safeReturnUrl("/\t/evil.example")).toBeNull();
    expect(safeReturnUrl("/%0A/evil.example")).toBeNull();
    expect(safeReturnUrl("/%00/evil.example")).toBeNull();
  });

  it("rejects dot segments that normalise to //host", () => {
    expect(safeReturnUrl("/.//evil.example")).toBeNull();
    expect(safeReturnUrl("/a/..//evil.example")).toBeNull();
    expect(safeReturnUrl("%2F.%2F%2Fevil.example")).toBeNull();
  });

  it("keeps the query and fragment of an in-app path", () => {
    expect(safeReturnUrl("/invite/abc?x=1#top")).toBe("/invite/abc?x=1#top");
  });

  it("returns null for malformed escapes instead of throwing", () => {
    expect(safeReturnUrl("%E0%A4%A")).toBeNull();
  });
});
