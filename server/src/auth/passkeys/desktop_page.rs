//! Server-rendered pages of the desktop passkey flow (F1, C29 steps 3–4):
//! the ceremony page, the "expired" page and the hand-off page that posts
//! the result to the app's loopback listener.

use std::sync::LazyLock;

use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Where the hand-off page posts: `http://127.0.0.1:{port}/callback/{path}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopbackTarget {
    pub port: u16,
    /// 32 random bytes, b64u (43 characters), drawn by the app per flow.
    pub callback_path: String,
}

impl LoopbackTarget {
    /// The literal loopback address, never `localhost` (F1).
    #[must_use]
    pub fn url(&self) -> String {
        format!(
            "http://127.0.0.1:{}/callback/{}",
            self.port, self.callback_path
        )
    }
}

/// The one field the hand-off page carries besides `flow_id`.
pub enum HandoffField<'a> {
    /// The one-time code (success).
    Code(&'a str),
    /// A failure code (`PASSKEY_CANCELLED` for a cancel).
    Error(&'a str),
}

/// The constant inline script of the hand-off page, allowed by its hash.
const HANDOFF_SCRIPT: &str = "document.getElementById(\"handoff\").submit();";

static HANDOFF_SCRIPT_SHA256: LazyLock<String> = LazyLock::new(|| {
    base64::engine::general_purpose::STANDARD.encode(Sha256::digest(HANDOFF_SCRIPT.as_bytes()))
});

/// Minimal HTML escaping for text and attribute values.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn no_store(mut response: Response) -> Response {
    let h = response.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}

/// The hand-off page (C29 step 4).
///
/// One form POST to the loopback URL with
/// `flow_id` and `code` or `error`, submitted by a hashed script, with a
/// "Continue" button without script. No other content, never a redirect.
/// L1b reuses it for the OIDC desktop delivery (C29 step 8).
pub fn handoff_page(loopback: &LoopbackTarget, flow_id: Uuid, field: HandoffField<'_>) -> Response {
    let url = loopback.url();
    let (name, value) = match field {
        HandoffField::Code(c) => ("code", c),
        HandoffField::Error(e) => ("error", e),
    };
    let html = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>Kaiku</title></head><body>\
         <form id=\"handoff\" method=\"post\" enctype=\"application/x-www-form-urlencoded\" action=\"{url}\">\
         <input type=\"hidden\" name=\"flow_id\" value=\"{flow}\">\
         <input type=\"hidden\" name=\"{name}\" value=\"{value}\">\
         <button type=\"submit\">Continue</button></form>\
         <script>{HANDOFF_SCRIPT}</script></body></html>",
        url = esc(&url),
        flow = flow_id,
        value = esc(value),
    );
    let csp = format!(
        "default-src 'none'; script-src 'sha256-{}'; form-action {url}; base-uri 'none'; \
         frame-ancestors 'none'",
        *HANDOFF_SCRIPT_SHA256
    );
    let mut response = (StatusCode::OK, axum::response::Html(html)).into_response();
    // Never serve the hand-off without its CSP: the port is a number and the
    // path is validated b64u, so the value is always a valid header.
    let csp = HeaderValue::from_str(&csp).expect("valid hand-off CSP header");
    response
        .headers_mut()
        .insert(header::CONTENT_SECURITY_POLICY, csp);
    no_store(response)
}

/// Page language from `Accept-Language`: German if it is the first
/// supported language named, else English (DC7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
}

impl Lang {
    #[must_use]
    pub fn from_headers(headers: &HeaderMap) -> Self {
        let raw = headers
            .get(header::ACCEPT_LANGUAGE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        for part in raw.split(',') {
            let tag = part
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            if tag == "de" || tag.starts_with("de-") {
                return Self::De;
            }
            if tag == "en" || tag.starts_with("en-") {
                return Self::En;
            }
        }
        Self::En
    }

    const fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::De => "de",
        }
    }
}

/// What the ceremony page asks the user to allow.
pub enum PageHeading {
    SignIn,
    RemovePasskey(String),
    RemoveDevice(String),
    Elevate,
    Other,
}

struct Texts {
    title_sign_in: &'static str,
    remove_passkey: (&'static str, &'static str),
    remove_device: (&'static str, &'static str),
    elevate: &'static str,
    other: &'static str,
    forwarded: &'static str,
    start: &'static str,
    back: &'static str,
    cancelled: &'static str,
    retry: &'static str,
    unsupported: &'static str,
    linux_firefox: &'static str,
    linux_other: &'static str,
    working: &'static str,
    expired: &'static str,
}

const EN: Texts = Texts {
    title_sign_in: "Sign in to the Kaiku desktop app on this computer",
    remove_passkey: (
        "Allow the Kaiku desktop app on this computer to remove the passkey \u{201c}",
        "\u{201d}",
    ),
    remove_device: (
        "Allow the Kaiku desktop app on this computer to remove the device \u{201c}",
        "\u{201d}",
    ),
    elevate: "Allow the Kaiku desktop app on this computer to unlock admin actions",
    other: "Allow the Kaiku desktop app on this computer to make this security change",
    forwarded: "If someone sent you this link, close this page.",
    start: "Continue with passkey",
    back: "Back to the app",
    cancelled: "Cancelled.",
    retry: "Try again",
    unsupported: "This browser cannot use passkeys. Go back to the app and choose another way.",
    linux_firefox: "This browser cannot use your phone. Use a security key, or choose a \
                    Chromium-based browser under \u{201c}Browser for passkey sign-in\u{201d} in \
                    the Kaiku app.",
    linux_other: "No phone option? Use a security key.",
    working: "Waiting for your passkey\u{2026}",
    expired: "This link has expired or was already used. Start again in the Kaiku app.",
};

const DE: Texts = Texts {
    title_sign_in: "Bei der Kaiku-Desktop-App auf diesem Computer anmelden",
    remove_passkey: (
        "Der Kaiku-Desktop-App auf diesem Computer erlauben, den Passkey \u{201e}",
        "\u{201c} zu entfernen",
    ),
    remove_device: (
        "Der Kaiku-Desktop-App auf diesem Computer erlauben, das Ger\u{e4}t \u{201e}",
        "\u{201c} zu entfernen",
    ),
    elevate: "Der Kaiku-Desktop-App auf diesem Computer erlauben, Admin-Aktionen freizuschalten",
    other: "Der Kaiku-Desktop-App auf diesem Computer erlauben, diese Sicherheits\u{e4}nderung \
            vorzunehmen",
    forwarded: "Wenn dir jemand diesen Link geschickt hat, schlie\u{df}e diese Seite.",
    start: "Mit Passkey fortfahren",
    back: "Zur\u{fc}ck zur App",
    cancelled: "Abgebrochen.",
    retry: "Nochmal versuchen",
    unsupported: "Dieser Browser kann keine Passkeys verwenden. Geh zur\u{fc}ck zur App und \
                  w\u{e4}hle einen anderen Weg.",
    linux_firefox: "Dieser Browser kann dein Handy nicht verwenden. Nutze einen \
                    Sicherheitsschl\u{fc}ssel oder w\u{e4}hle in der Kaiku-App unter \
                    \u{201e}Browser f\u{fc}r die Passkey-Anmeldung\u{201c} einen Chromium-basierten \
                    Browser.",
    linux_other: "Keine Handy-Option? Nutze einen Sicherheitsschl\u{fc}ssel.",
    working: "Warte auf deinen Passkey\u{2026}",
    expired: "Dieser Link ist abgelaufen oder wurde schon benutzt. Starte in der Kaiku-App neu.",
};

const fn texts(lang: Lang) -> &'static Texts {
    match lang {
        Lang::En => &EN,
        Lang::De => &DE,
    }
}

/// The ceremony page's CSP: only its own script and stylesheet.
const PAGE_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; connect-src \
                        'self'; img-src 'self'; form-action 'self'; base-uri 'none'; \
                        frame-ancestors 'none'";

fn page(lang: Lang, status: StatusCode, body: &str) -> Response {
    let html = format!(
        "<!doctype html><html lang=\"{lang}\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>Kaiku</title>\
         <link rel=\"stylesheet\" href=\"/auth/passkeys/assets/ceremony.css\"></head>\
         <body><main>{body}</main></body></html>",
        lang = lang.code()
    );
    let mut response = (status, axum::response::Html(html)).into_response();
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(PAGE_CSP),
    );
    no_store(response)
}

/// 404: an unknown flow, or one that already holds a result.
pub fn expired_page(lang: Lang) -> Response {
    let t = texts(lang);
    page(
        lang,
        StatusCode::NOT_FOUND,
        &format!("<h1>Kaiku</h1><p>{}</p>", esc(t.expired)),
    )
}

/// 400: a finish without a live ceremony (a second finish, or a reload race).
pub fn stale_page(lang: Lang) -> Response {
    let t = texts(lang);
    page(
        lang,
        StatusCode::BAD_REQUEST,
        &format!("<h1>Kaiku</h1><p>{}</p>", esc(t.expired)),
    )
}

/// The ceremony page.
///
/// It names the purpose and the app, warns about forwarded
/// links, and runs `ceremony.js` (options → `navigator.credentials.get` →
/// a form POST of the credential to `finish`; "Back to the app" posts
/// `cancel`).
pub fn ceremony_page(lang: Lang, heading: &PageHeading, flow_id: Uuid) -> Response {
    let t = texts(lang);
    let title = match heading {
        PageHeading::SignIn => t.title_sign_in.to_string(),
        PageHeading::RemovePasskey(label) => {
            format!("{}{}{}", t.remove_passkey.0, label, t.remove_passkey.1)
        }
        PageHeading::RemoveDevice(label) => {
            format!("{}{}{}", t.remove_device.0, label, t.remove_device.1)
        }
        PageHeading::Elevate => t.elevate.to_string(),
        PageHeading::Other => t.other.to_string(),
    };
    let body = format!(
        "<h1>{title}</h1>\
         <p class=\"warning\">{forwarded}</p>\
         <p id=\"status\" role=\"status\" aria-live=\"polite\" data-working=\"{working}\" \
            data-cancelled=\"{cancelled}\"></p>\
         <p id=\"unsupported\" hidden>{unsupported}</p>\
         <p id=\"linux-firefox\" class=\"hint\" hidden>{linux_firefox}</p>\
         <p id=\"linux-other\" class=\"hint\" hidden>{linux_other}</p>\
         <button id=\"start\" type=\"button\" data-retry=\"{retry}\">{start}</button>\
         <form id=\"finish\" method=\"post\" action=\"{base}/finish\" hidden>\
         <input type=\"hidden\" name=\"credential\" value=\"\">\
         <input type=\"hidden\" name=\"ceremony_id\" value=\"\"></form>\
         <form id=\"cancel\" method=\"post\" action=\"{base}/cancel\">\
         <button type=\"submit\" class=\"secondary\">{back}</button></form>\
         <script src=\"/auth/passkeys/assets/ceremony.js\"></script>",
        title = esc(&title),
        forwarded = esc(t.forwarded),
        working = esc(t.working),
        cancelled = esc(t.cancelled),
        unsupported = esc(t.unsupported),
        linux_firefox = esc(t.linux_firefox),
        linux_other = esc(t.linux_other),
        retry = esc(t.retry),
        start = esc(t.start),
        back = esc(t.back),
        base = format_args!("/auth/passkeys/desktop/{flow_id}"),
    );
    page(lang, StatusCode::OK, &body)
}

/// `GET /auth/passkeys/assets/ceremony.js`.
pub async fn ceremony_js() -> Response {
    asset(
        include_str!("assets/ceremony.js"),
        "text/javascript; charset=utf-8",
    )
}

/// `GET /auth/passkeys/assets/ceremony.css`.
pub async fn ceremony_css() -> Response {
    asset(
        include_str!("assets/ceremony.css"),
        "text/css; charset=utf-8",
    )
}

fn asset(body: &'static str, content_type: &'static str) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        body,
    )
        .into_response()
}
