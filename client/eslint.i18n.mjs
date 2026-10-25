// Files whose UI is fully migrated to t(); user-facing literals are lint errors here.
export const I18N_GUARDED_FILES = [
  "src/i18n/**/*.{ts,tsx}",
  "src/components/layout/ConnectionIndicator.tsx",
  "src/components/settings/QrLoginModal.tsx",
  "src/components/settings/LanguageSelect.tsx",
  "src/components/home/DMConversation.tsx",
  "src/components/admin/CreateAccountModal.tsx",
  "src/views/ForcedPasswordChange.tsx",
];

const MESSAGE = "User-facing text must use t() from @/i18n (see client/src/AGENTS.md).";

const TAURI_MESSAGE = "Use isTauri() from @/lib/tauri/common (contract C6).";

// Tauri detection goes through isTauri() only; the global is absent when
// withGlobalTauri is false, so a hand-rolled check silently means "browser".
export const TAURI_DETECTION_SELECTORS = [
  { selector: "Literal[value='__TAURI__']", message: TAURI_MESSAGE },
  { selector: "MemberExpression[property.name='__TAURI__']", message: TAURI_MESSAGE },
];

// Direct-child chains only, so t("...") arguments and class={x ? "a" : "b"}
// never match: the container must sit in JSX children or in a text attribute.
// `parent` is the selector for the container's parent / ancestor chain.
const TEXT = "/[A-Za-z]/";
// Wrapper nodes that pass text through: ternaries, `||`/`??`/`&&`, and `+` concatenation.
// Up to three nested wrappers deep, e.g. `a ? "x: " + e : "y"`.
const WRAPPER = ":matches(ConditionalExpression, LogicalExpression, BinaryExpression[operator='+'])";
const textLiterals = (prefix, literalFilter = "") => {
  const out = [];
  for (let depth = 0; depth <= 3; depth++) {
    const chain = `${prefix}${` > ${WRAPPER}`.repeat(depth)}`;
    out.push(`${chain} > Literal[value=${TEXT}]${literalFilter}`);
    out.push(`${chain} > TemplateLiteral > TemplateElement[value.raw=${TEXT}]`);
  }
  return out;
};
const KEY_SHAPE = "/^[a-z][a-zA-Z0-9]*(\\.[a-zA-Z0-9]+)+$/";
const TOAST_PROP =
  "CallExpression[callee.name='showToast'] > ObjectExpression > Property[key.name=/^(title|message)$/]";
const SETTER_CALL = "CallExpression[callee.name=/^set\\w*(Error|Message)$/]";
const CHILD_CONTAINER = ":matches(JSXElement, JSXFragment) > JSXExpressionContainer";
const ATTR_CONTAINER =
  "JSXAttribute[name.name=/^(placeholder|title|aria-label|aria-description|alt|label)$/] > JSXExpressionContainer";

// A file-specific no-restricted-syntax block replaces the global one, so the
// Tauri detection ban is repeated here.
export const I18N_SELECTORS = [
  ...[
    ...textLiterals(CHILD_CONTAINER),
    ...textLiterals(ATTR_CONTAINER),
    ...textLiterals(TOAST_PROP),
    // setError("auth.some.key") passes a MessageKey, not text: key-shaped literals are fine.
    ...textLiterals(SETTER_CALL, `:not([value=${KEY_SHAPE}])`),
  ].map((selector) => ({ selector, message: MESSAGE })),
  {
    selector: `${CHILD_CONTAINER} > Literal[value=' ']`,
    message: 'Don\'t join sentence fragments with {" "}; use one whole-sentence key.',
  },
  ...TAURI_DETECTION_SELECTORS,
  { selector: "JSXText[value=/[A-Za-z]/]", message: MESSAGE },
  {
    selector: "JSXAttribute[name.name=/^(placeholder|title|aria-label|aria-description|alt|label)$/] > Literal",
    message: MESSAGE,
  },
];
