import js from "@eslint/js";
import tseslint from "typescript-eslint";
import solid from "eslint-plugin-solid/configs/typescript";

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
    },
  },
  {
    ignores: ["node_modules/", "dist/", "src-tauri/"],
  },
];
