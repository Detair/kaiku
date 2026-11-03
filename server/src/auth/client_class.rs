//! Client class from `X-Kaiku-Client` (C26 common rules, B.4 lifetimes).

use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};

/// Header the native clients send on every request.
pub const CLIENT_HEADER: &str = "x-kaiku-client";

/// Which kind of client a request comes from. Decides the absolute session
/// cap (B.4) and whether secrets go into response bodies (native) or
/// cookies (web).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientClass {
    Web,
    Desktop,
    Android,
}

impl ClientClass {
    /// `X-Kaiku-Client`: exactly `desktop` → Desktop, exactly `android` →
    /// Android, anything else (or none) → Web.
    #[must_use]
    pub fn from_headers(h: &HeaderMap) -> Self {
        match h.get(CLIENT_HEADER).map(axum::http::HeaderValue::as_bytes) {
            Some(b"desktop") => Self::Desktop,
            Some(b"android") => Self::Android,
            _ => Self::Web,
        }
    }

    /// Parse the stored / wire form.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "web" => Some(Self::Web),
            "desktop" => Some(Self::Desktop),
            "android" => Some(Self::Android),
            _ => None,
        }
    }

    /// The stored and wire form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Desktop => "desktop",
            Self::Android => "android",
        }
    }

    /// The byte inside signed contexts (C26): web 1, desktop 2, android 3.
    #[must_use]
    pub const fn byte(self) -> u8 {
        match self {
            Self::Web => 1,
            Self::Desktop => 2,
            Self::Android => 3,
        }
    }

    /// Inverse of [`Self::byte`].
    #[must_use]
    pub const fn from_byte(b: u8) -> Option<Self> {
        match b {
            1 => Some(Self::Web),
            2 => Some(Self::Desktop),
            3 => Some(Self::Android),
            _ => None,
        }
    }

    /// Absolute session lifetime from `created_at` (B.4): browser 30 d,
    /// desktop and Android 90 d.
    #[must_use]
    pub const fn absolute_cap(self) -> chrono::Duration {
        match self {
            Self::Web => chrono::Duration::days(30),
            Self::Desktop | Self::Android => chrono::Duration::days(90),
        }
    }

    /// Native clients get tokens and secrets in response bodies.
    #[must_use]
    pub const fn is_native(self) -> bool {
        matches!(self, Self::Desktop | Self::Android)
    }
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};

    use super::*;

    #[test]
    fn client_class_from_header() {
        let mut h = HeaderMap::new();
        assert_eq!(ClientClass::from_headers(&h), ClientClass::Web);
        h.insert("x-kaiku-client", HeaderValue::from_static("desktop"));
        assert_eq!(ClientClass::from_headers(&h), ClientClass::Desktop);
        h.insert("x-kaiku-client", HeaderValue::from_static("android"));
        assert_eq!(ClientClass::from_headers(&h), ClientClass::Android);
        for other in ["Desktop", "android ", "ios", ""] {
            h.insert("x-kaiku-client", HeaderValue::from_str(other).unwrap());
            assert_eq!(ClientClass::from_headers(&h), ClientClass::Web, "{other:?}");
        }
        assert_eq!(ClientClass::Web.byte(), 1);
        assert_eq!(ClientClass::Desktop.byte(), 2);
        assert_eq!(ClientClass::Android.byte(), 3);
        assert_eq!(ClientClass::Web.absolute_cap(), chrono::Duration::days(30));
        assert_eq!(
            ClientClass::Desktop.absolute_cap(),
            chrono::Duration::days(90)
        );
        assert_eq!(
            ClientClass::Android.absolute_cap(),
            chrono::Duration::days(90)
        );
        assert!(!ClientClass::Web.is_native());
        assert!(ClientClass::Desktop.is_native() && ClientClass::Android.is_native());
        assert_eq!(ClientClass::Android.as_str(), "android");
    }
}
