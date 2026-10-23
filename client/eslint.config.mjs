import js from "@eslint/js";
import tseslint from "typescript-eslint";
import solid from "eslint-plugin-solid/configs/typescript";
import {
  I18N_GUARDED_FILES,
  I18N_SELECTORS,
  TAURI_DETECTION_SELECTORS,
} from "./eslint.i18n.mjs";

export default [
  js.configs.recommended,
  ...tseslint.configs.recommended,
  solid,
  {
    files: ["src/**/*.{ts,tsx}"],
    languageOptions: {
      parserOptions: {
        project: true,
      },
    },
    rules: {
      "@typescript-eslint/no-unused-vars": [
        "warn",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
      ],
      "@typescript-eslint/no-explicit-any": "warn",
      "no-console": "off",
      // SolidJS ref pattern (`let myRef: T | undefined;` + `<div ref={myRef}>`)
      // defeats this rule's static analysis — the JSX ref={} binding form
      // assigns the variable at render time, which ESLint can't trace.
      "no-unassigned-vars": "off",
      "no-restricted-syntax": ["error", ...TAURI_DETECTION_SELECTORS],
    },
  },
  {
    files: I18N_GUARDED_FILES,
    // No type-aware rules are enabled, so skip the project lookup; this also lets
    // the lint test run on a virtual path that is not in any tsconfig.
    languageOptions: { parserOptions: { project: false } },
    rules: {
      "no-restricted-syntax": ["error", ...I18N_SELECTORS],
    },
  },
  {
    ignores: ["node_modules/", "dist/", "src-tauri/"],
  },
];
