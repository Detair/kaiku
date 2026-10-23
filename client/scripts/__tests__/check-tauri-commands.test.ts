import { describe, expect, it } from "vitest";
import {
  findInvokedCommands,
  findNonLiteralInvokes,
  findRegisteredCommands,
  findUncalledIsTauri,
  findUnregisteredInvokes,
} from "../check-tauri-commands";

const LIB_RS = `
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            // Auth commands
            commands::auth::login,
            commands::auth::logout,
            commands::tray::tray_set_unread,
        ])
        .run(tauri::generate_context!())
}
`;

function files(entries: Record<string, string>): Map<string, string> {
  return new Map(Object.entries(entries));
}

describe("check-tauri-commands", () => {
  it("reports an invoke name missing from generate_handler", () => {
    const src = files({
      "src/lib/tauri/guilds.ts": `
        const { invoke } = await import("@tauri-apps/api/core");
        return invoke("get_guilds");
      `,
      "src/lib/tauri/auth.ts": `await invoke("logout");`,
    });

    expect([...findInvokedCommands(src)].sort()).toEqual(["get_guilds", "logout"]);
    expect(findUnregisteredInvokes(src, LIB_RS).map((u) => u.name)).toEqual([
      "get_guilds",
    ]);
    expect(findUnregisteredInvokes(src, LIB_RS)[0].files).toEqual([
      "src/lib/tauri/guilds.ts",
    ]);
  });

  it("accepts registered names with type params", () => {
    const src = files({
      "src/lib/tauri/auth.ts": `
        const r = await invoke<AuthResult>("login", { serverUrl });
        const s = await invoke<Record<string, Array<() => void>>>(
          'tray_set_unread',
          { count },
        );
      `,
    });

    expect([...findInvokedCommands(src)].sort()).toEqual([
      "login",
      "tray_set_unread",
    ]);
    expect(findUnregisteredInvokes(src, LIB_RS)).toEqual([]);
    expect(findNonLiteralInvokes(src)).toEqual([]);
  });

  it("rejects non-literal invoke names", () => {
    const src = files({
      "src/lib/x.ts": `
        const cmd = "login";
        await invoke(cmd);
        await invoke<void>(\`tray_\${kind}\`);
      `,
    });

    const bad = findNonLiteralInvokes(src);
    expect(bad.map((b) => b.line)).toEqual([3, 4]);
    expect(bad.every((b) => b.file === "src/lib/x.ts")).toBe(true);
    expect([...findInvokedCommands(src)]).toEqual([]);
  });

  it("ignores invoke mentions in comments, strings, imports and tests", () => {
    const src = files({
      "src/lib/a.ts": `
        import { invoke } from "@tauri-apps/api/core";
        // invoke("get_guilds") was the old path
        /* invoke("get_dm_list") */
        const s = 'invoke("get_messages")';
        const t = typeof invoke;
      `,
      "src/lib/__tests__/a.test.ts": `invoke("get_guilds");`,
    });

    expect([...findInvokedCommands(src)]).toEqual([]);
    expect(findNonLiteralInvokes(src)).toEqual([]);
  });

  it("collects registered command identifiers", () => {
    expect([...findRegisteredCommands(LIB_RS)].sort()).toEqual([
      "login",
      "logout",
      "tray_set_unread",
    ]);
  });

  it("flags an uncalled isTauri", () => {
    const src = files({
      "src/lib/a.ts": [
        `import { isTauri } from "./common";`,
        `if (isTauri()) {}`,
        `if (isTauri) {}`,
        `export { isTauri } from "./common";`,
      ].join("\n"),
    });

    expect(findUncalledIsTauri(src)).toEqual(["src/lib/a.ts:3"]);
  });
});
