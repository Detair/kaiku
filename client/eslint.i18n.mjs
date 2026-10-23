// Files whose UI is fully migrated to t(); user-facing literals are lint errors here.
export const I18N_GUARDED_FILES = [
  "src/i18n/**/*.{ts,tsx}",
  "src/components/layout/ConnectionIndicator.tsx",
  "src/components/settings/QrLoginModal.tsx",
  "src/components/home/DMConversation.tsx",
];

const MESSAGE = "User-facing text must use t() from @/i18n (see client/src/AGENTS.md).";

const TAURI_MESSAGE = "Use isTauri() from @/lib/tauri/common (contract C6).";

// Tauri detection goes through isTauri() only; the global is absent when
// withGlobalTauri is false, so a hand-rolled check silently means "browser".
export const TAURI_DETECTION_SELECTORS = [
  { selector: "Literal[value='__TAURI__']", message: TAURI_MESSAGE },
  { selector: "MemberExpression[property.name='__TAURI__']", message: TAURI_MESSAGE },
];

// A file-specific no-restricted-syntax block replaces the global one, so the
// Tauri detection ban is repeated here.
export const I18N_SELECTORS = [
  ...TAURI_DETECTION_SELECTORS,
  { selector: "JSXText[value=/[A-Za-z]/]", message: MESSAGE },
  {
    selector: "JSXAttribute[name.name=/^(placeholder|title|aria-label|alt|label)$/] > Literal",
    message: MESSAGE,
  },
];
