//! Canonical contexts C (contract C26). Field order, byte values and
//! encodings are binding; the JSON `context` uses exactly these field names.
//!
//! Strings are hashed as their exact UTF-8 bytes, without normalisation or
//! trimming, so the device recomputes the same hash from the JSON it shows
//! (spec B.2: the signature covers exactly what the phone showed).

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::secrets::{b64u_decode, b64u_encode};
use super::signing::{context_hash, lp, Kind};
use crate::auth::client_class::ClientClass;

/// `login` variants (byte, JSON).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Variant {
    Qr = 1,
    Password = 2,
    Oidc = 3,
}

impl Variant {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Qr => "qr",
            Self::Password => "password",
            Self::Oidc => "oidc",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "qr" => Some(Self::Qr),
            "password" => Some(Self::Password),
            "oidc" => Some(Self::Oidc),
            _ => None,
        }
    }
}

/// `enroll` variants (byte, JSON).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EnrollVariant {
    Own = 1,
    Admin = 2,
}

impl EnrollVariant {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Own => "own",
            Self::Admin => "admin",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "own" => Some(Self::Own),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }
}

/// What an enrolment adds (byte, JSON).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AuthenticatorType {
    KeyDevice = 1,
    Passkey = 2,
}

impl AuthenticatorType {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::KeyDevice => "key_device",
            Self::Passkey => "passkey",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "key_device" => Some(Self::KeyDevice),
            "passkey" => Some(Self::Passkey),
            _ => None,
        }
    }
}

/// How an admin checked the claimant (byte, JSON); `none` for `own`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ClaimantCheck {
    None = 0,
    InPerson = 1,
    CalledBack = 2,
}

impl ClaimantCheck {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::InPerson => "in_person",
            Self::CalledBack => "called_back",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "none" => Some(Self::None),
            "in_person" => Some(Self::InPerson),
            "called_back" => Some(Self::CalledBack),
            _ => None,
        }
    }
}

/// `login`: `attempt_id ‖ collection_hash ‖ variant ‖ client_class ‖
/// lp(client_label) ‖ i64(created_at) ‖ lp(server_name) ‖ lp(place) ‖
/// lp(username)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoginContext {
    pub attempt_id: Uuid,
    pub collection_hash: [u8; 32],
    pub variant: Variant,
    pub client_class: ClientClass,
    pub client_label: String,
    pub created_at: i64,
    pub server_name: String,
    pub place: String,
    pub username: String,
}

/// `enroll`: `enrollment_id ‖ subject_id ‖ lp(subject_username) ‖ variant ‖
/// authenticator_type ‖ new_key_hash ‖ lp(label) ‖ claimant_check`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnrollContext {
    pub enrollment_id: Uuid,
    pub subject_id: Uuid,
    pub subject_username: String,
    pub variant: EnrollVariant,
    pub authenticator_type: AuthenticatorType,
    pub new_key_hash: [u8; 32],
    pub label: String,
    pub claimant_check: ClaimantCheck,
}

/// `device_stepup`: `sid ‖ lp(action) ‖ target_hash ‖ lp(target_label)`;
/// the JSON carries `target`, the device computes `target_hash`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceStepupContext {
    pub sid: Uuid,
    pub action: String,
    pub target: String,
    pub target_label: String,
}

/// `enroll_pop`: `enrollment_id ‖ new_key_hash ‖ attestation_challenge ‖
/// lp(subject_username)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnrollPopContext {
    pub enrollment_id: Uuid,
    pub new_key_hash: [u8; 32],
    pub attestation_challenge: [u8; 32],
    pub subject_username: String,
}

/// One canonical context per kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalContext {
    Login(LoginContext),
    Enroll(EnrollContext),
    DeviceLogin { client_class: ClientClass },
    DeviceStepup(DeviceStepupContext),
    EnrollPop(EnrollPopContext),
}

/// A JSON context that does not have the shape of its kind.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("context field `{0}` is missing or invalid")]
pub struct ContextError(pub &'static str);

/// `SHA-256(lp(target))` (C27 common rules).
#[must_use]
pub fn target_hash(target: &str) -> [u8; 32] {
    let mut buf = Vec::with_capacity(4 + target.len());
    lp(&mut buf, target.as_bytes());
    Sha256::digest(&buf).into()
}

impl CanonicalContext {
    /// The kind this context belongs to.
    #[must_use]
    pub const fn kind(&self) -> Kind {
        match self {
            Self::Login(_) => Kind::Login,
            Self::Enroll(_) => Kind::Enroll,
            Self::DeviceLogin { .. } => Kind::DeviceLogin,
            Self::DeviceStepup(_) => Kind::DeviceStepup,
            Self::EnrollPop(_) => Kind::EnrollPop,
        }
    }

    /// The canonical bytes C.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(128);
        match self {
            Self::Login(c) => {
                out.extend_from_slice(c.attempt_id.as_bytes());
                out.extend_from_slice(&c.collection_hash);
                out.push(c.variant as u8);
                out.push(c.client_class.byte());
                lp(&mut out, c.client_label.as_bytes());
                out.extend_from_slice(&c.created_at.to_be_bytes());
                lp(&mut out, c.server_name.as_bytes());
                lp(&mut out, c.place.as_bytes());
                lp(&mut out, c.username.as_bytes());
            }
            Self::Enroll(c) => {
                out.extend_from_slice(c.enrollment_id.as_bytes());
                out.extend_from_slice(c.subject_id.as_bytes());
                lp(&mut out, c.subject_username.as_bytes());
                out.push(c.variant as u8);
                out.push(c.authenticator_type as u8);
                out.extend_from_slice(&c.new_key_hash);
                lp(&mut out, c.label.as_bytes());
                out.push(c.claimant_check as u8);
            }
            Self::DeviceLogin { client_class } => out.push(client_class.byte()),
            Self::DeviceStepup(c) => {
                out.extend_from_slice(c.sid.as_bytes());
                lp(&mut out, c.action.as_bytes());
                out.extend_from_slice(&target_hash(&c.target));
                lp(&mut out, c.target_label.as_bytes());
            }
            Self::EnrollPop(c) => {
                out.extend_from_slice(c.enrollment_id.as_bytes());
                out.extend_from_slice(&c.new_key_hash);
                out.extend_from_slice(&c.attestation_challenge);
                lp(&mut out, c.subject_username.as_bytes());
            }
        }
        out
    }

    /// `context_hash` = SHA-256(`"kaiku-sig-context-v1" ‖ kind ‖ C`).
    #[must_use]
    pub fn hash(&self) -> [u8; 32] {
        context_hash(self.kind(), &self.encode())
    }

    /// The JSON `context` sent with a challenge (C26 field names; hashes
    /// b64u, ids lowercase hyphenated, `created_at` integer seconds).
    #[must_use]
    pub fn to_json(&self) -> Value {
        match self {
            Self::Login(c) => json!({
                "attempt_id": c.attempt_id,
                "collection_hash": b64u_encode(&c.collection_hash),
                "variant": c.variant.as_str(),
                "client_class": c.client_class.as_str(),
                "client_label": c.client_label,
                "created_at": c.created_at,
                "server_name": c.server_name,
                "place": c.place,
                "username": c.username,
            }),
            Self::Enroll(c) => json!({
                "enrollment_id": c.enrollment_id,
                "subject_id": c.subject_id,
                "subject_username": c.subject_username,
                "variant": c.variant.as_str(),
                "authenticator_type": c.authenticator_type.as_str(),
                "new_key_hash": b64u_encode(&c.new_key_hash),
                "label": c.label,
                "claimant_check": c.claimant_check.as_str(),
            }),
            Self::DeviceLogin { client_class } => json!({ "client_class": client_class.as_str() }),
            Self::DeviceStepup(c) => json!({
                "sid": c.sid,
                "action": c.action,
                "target": c.target,
                "target_label": c.target_label,
            }),
            Self::EnrollPop(c) => json!({
                "enrollment_id": c.enrollment_id,
                "new_key_hash": b64u_encode(&c.new_key_hash),
                "attestation_challenge": b64u_encode(&c.attestation_challenge),
                "subject_username": c.subject_username,
            }),
        }
    }

    /// Parse a JSON `context` of `kind`. Unknown extra fields are ignored;
    /// missing or mistyped fields are refused.
    pub fn from_json(kind: Kind, v: &Value) -> Result<Self, ContextError> {
        let m = v.as_object().ok_or(ContextError("context"))?;
        Ok(match kind {
            Kind::Login => Self::Login(LoginContext {
                attempt_id: uuid_field(m, "attempt_id")?,
                collection_hash: hash_field(m, "collection_hash")?,
                variant: Variant::parse(str_field(m, "variant")?).ok_or(ContextError("variant"))?,
                client_class: class_field(m)?,
                client_label: str_field(m, "client_label")?.to_owned(),
                created_at: m
                    .get("created_at")
                    .and_then(Value::as_i64)
                    .ok_or(ContextError("created_at"))?,
                server_name: str_field(m, "server_name")?.to_owned(),
                place: str_field(m, "place")?.to_owned(),
                username: str_field(m, "username")?.to_owned(),
            }),
            Kind::Enroll => Self::Enroll(EnrollContext {
                enrollment_id: uuid_field(m, "enrollment_id")?,
                subject_id: uuid_field(m, "subject_id")?,
                subject_username: str_field(m, "subject_username")?.to_owned(),
                variant: EnrollVariant::parse(str_field(m, "variant")?)
                    .ok_or(ContextError("variant"))?,
                authenticator_type: AuthenticatorType::parse(str_field(m, "authenticator_type")?)
                    .ok_or(ContextError("authenticator_type"))?,
                new_key_hash: hash_field(m, "new_key_hash")?,
                label: str_field(m, "label")?.to_owned(),
                claimant_check: ClaimantCheck::parse(str_field(m, "claimant_check")?)
                    .ok_or(ContextError("claimant_check"))?,
            }),
            Kind::DeviceLogin => Self::DeviceLogin {
                client_class: class_field(m)?,
            },
            Kind::DeviceStepup => Self::DeviceStepup(DeviceStepupContext {
                sid: uuid_field(m, "sid")?,
                action: str_field(m, "action")?.to_owned(),
                target: str_field(m, "target")?.to_owned(),
                target_label: str_field(m, "target_label")?.to_owned(),
            }),
            Kind::EnrollPop => Self::EnrollPop(EnrollPopContext {
                enrollment_id: uuid_field(m, "enrollment_id")?,
                new_key_hash: hash_field(m, "new_key_hash")?,
                attestation_challenge: hash_field(m, "attestation_challenge")?,
                subject_username: str_field(m, "subject_username")?.to_owned(),
            }),
        })
    }
}

fn str_field<'a>(m: &'a Map<String, Value>, k: &'static str) -> Result<&'a str, ContextError> {
    m.get(k).and_then(Value::as_str).ok_or(ContextError(k))
}

fn uuid_field(m: &Map<String, Value>, k: &'static str) -> Result<Uuid, ContextError> {
    Uuid::parse_str(str_field(m, k)?).map_err(|_| ContextError(k))
}

fn hash_field(m: &Map<String, Value>, k: &'static str) -> Result<[u8; 32], ContextError> {
    b64u_decode(str_field(m, k)?)
        .and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok())
        .ok_or(ContextError(k))
}

fn class_field(m: &Map<String, Value>) -> Result<ClientClass, ContextError> {
    ClientClass::parse(str_field(m, "client_class")?).ok_or(ContextError("client_class"))
}
