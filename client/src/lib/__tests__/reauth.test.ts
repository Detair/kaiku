import { describe, it, expect } from "vitest";
import { methodsFor, defaultReauthStrategy } from "../reauth";

const kc = [{ id: "kc", label: "Keycloak" }];

describe("methodsFor", () => {
  it("password-only", () => {
    expect(
      methodsFor({ hasLocalPassword: true, oidcProviders: [], oidcReauthSupported: false }),
    ).toEqual([{ kind: "password" }]);
  });

  it("OIDC-only stays hidden until the server defines the flow", () => {
    expect(
      methodsFor({ hasLocalPassword: false, oidcProviders: kc, oidcReauthSupported: false }),
    ).toEqual([]);
  });

  it("OIDC-only once supported", () => {
    expect(
      methodsFor({ hasLocalPassword: false, oidcProviders: kc, oidcReauthSupported: true }),
    ).toEqual([{ kind: "oidc", providerId: "kc", label: "Keycloak" }]);
  });

  it("both", () => {
    expect(
      methodsFor({ hasLocalPassword: true, oidcProviders: kc, oidcReauthSupported: true }),
    ).toEqual([
      { kind: "password" },
      { kind: "oidc", providerId: "kc", label: "Keycloak" },
    ]);
  });

  it("default strategy offers password only today", () => {
    expect(defaultReauthStrategy.availableMethods()).toEqual([{ kind: "password" }]);
  });
});
