// Solid.js test cleanup — ensures reactive roots are disposed between tests
import { afterEach } from "vitest";
import { cleanup } from "@solidjs/testing-library";

// Ensure window is available in test environment (for jsdom)
if (typeof window === "undefined") {
  // @ts-ignore - global window setup for tests
  global.window = {
    setTimeout: (fn: () => void, delay: number) => setTimeout(fn, delay),
    clearTimeout: (id: number) => clearTimeout(id),
    dispatchEvent: () => true,
    CustomEvent: class CustomEvent {},
  };
}

afterEach(() => {
  cleanup();
});
