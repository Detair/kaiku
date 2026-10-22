// Files whose UI is fully migrated to t(); user-facing literals are lint errors here.
export const I18N_GUARDED_FILES = ["src/i18n/**/*.{ts,tsx}"];

const MESSAGE = "User-facing text must use t() from @/i18n (see client/src/AGENTS.md).";

export const I18N_SELECTORS = [
  { selector: "JSXText[value=/[A-Za-z]/]", message: MESSAGE },
  {
    selector: "JSXAttribute[name.name=/^(placeholder|title|aria-label|alt|label)$/] > Literal",
    message: MESSAGE,
  },
];
