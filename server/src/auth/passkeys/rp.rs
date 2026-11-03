//! `webauthn-rs` in Kaiku's passkey profile (B.1, C29).

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use webauthn_rs::prelude::{
    AuthenticationResult, CreationChallengeResponse, CredentialID, DiscoverableAuthentication,
    Passkey, PasskeyAuthentication, PasskeyRegistration, PublicKeyCredential,
    RegisterPublicKeyCredential, RequestChallengeResponse,
};

use super::config::PasskeyConfig;

/// Why a ceremony step failed. Clients only ever see the mapped error code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasskeyFailure {
    /// Origin, RP ID, UV, BE, challenge or signature (`PASSKEY_INVALID`).
    Invalid,
    /// `credProps.rk == false` at registration (`PASSKEY_NOT_DISCOVERABLE`).
    NotDiscoverable,
    /// A discoverable assertion without a user handle.
    UserHandleMissing,
    Internal(String),
}

/// A verified registration, ready to store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiedRegistration {
    pub passkey: Passkey,
    pub aaguid: Option<Uuid>,
    pub backup_eligible: bool,
    pub backup_state: bool,
    pub transports: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PasskeyRp {
    config: PasskeyConfig,
    webauthn: webauthn_rs::Webauthn,
    allowed_origins: Vec<String>,
}

impl PasskeyRp {
    /// `rp_id` = the `PUBLIC_URL` host, the web origin plus every Android
    /// origin; subdomains and any-port stay off. The exact-string allow-list
    /// holds the Android origins first, then the web origin.
    pub fn new(config: PasskeyConfig) -> anyhow::Result<Self> {
        let android = config.android_origins();
        let mut builder = webauthn_rs::WebauthnBuilder::new(&config.rp_id, &config.web_origin)
            .map_err(|e| anyhow::anyhow!("passkey RP configuration refused: {e}"))?
            .rp_name("Kaiku")
            .timeout(std::time::Duration::from_mins(5))
            .allow_subdomains(false)
            .allow_any_port(false);
        for origin in &android {
            let url = url::Url::parse(origin).map_err(|_| {
                anyhow::anyhow!("PASSKEY_ANDROID_CERT_SHA256 gives an invalid origin")
            })?;
            builder = builder.append_allowed_origin(&url);
        }
        let webauthn = builder
            .build()
            .map_err(|e| anyhow::anyhow!("passkey RP configuration refused: {e}"))?;
        let mut allowed_origins = android;
        allowed_origins.push(config.web_origin.origin().ascii_serialization());
        Ok(Self {
            config,
            webauthn,
            allowed_origins,
        })
    }

    #[must_use]
    pub const fn config(&self) -> &PasskeyConfig {
        &self.config
    }

    #[must_use]
    pub fn rp_id(&self) -> &str {
        &self.config.rp_id
    }

    #[must_use]
    pub fn allowed_origins(&self) -> &[String] {
        &self.allowed_origins
    }

    /// The exact-string origin check (C29: Android origins first, then the
    /// web origin), on the raw `clientDataJSON` before the crate parses it.
    pub(crate) fn check_origin(&self, client_data_json: &[u8]) -> Result<(), PasskeyFailure> {
        let origin = serde_json::from_slice::<serde_json::Value>(client_data_json)
            .ok()
            .and_then(|v| v.get("origin").and_then(|o| o.as_str().map(str::to_owned)))
            .ok_or(PasskeyFailure::Invalid)?;
        if self.allowed_origins.contains(&origin) {
            Ok(())
        } else {
            tracing::debug!("passkey assertion from an origin outside the allow-list");
            Err(PasskeyFailure::Invalid)
        }
    }

    /// Creation options in the profile: `residentKey: required`, UV required,
    /// attestation `none`, `excludeCredentials`, `credProps`.
    pub fn start_registration(
        &self,
        user_handle: Uuid,
        username: &str,
        display_name: &str,
        exclude: Vec<CredentialID>,
    ) -> Result<(CreationChallengeResponse, PasskeyRegistration), PasskeyFailure> {
        let (ccr, state) = self
            .webauthn
            .start_passkey_registration(user_handle, username, display_name, Some(exclude))
            .map_err(internal)?;
        // webauthn-rs asks for `residentKey: discouraged`; the profile needs a
        // discoverable credential. The registration state ignores resident-key
        // settings at finish, so only the options change.
        let mut json = serde_json::to_value(&ccr).map_err(internal)?;
        json["publicKey"]["authenticatorSelection"] = serde_json::json!({
            "residentKey": "required",
            "requireResidentKey": true,
            "userVerification": "required",
        });
        let ccr = serde_json::from_value(json).map_err(internal)?;
        Ok((ccr, state))
    }

    /// Verify a registration: origin (exact), RP ID, UV, challenge; refuse
    /// `credProps.rk == false` (DC9). Reads BE/BS, transports and the AAGUID.
    pub fn finish_registration(
        &self,
        reg: &RegisterPublicKeyCredential,
        state: &PasskeyRegistration,
    ) -> Result<VerifiedRegistration, PasskeyFailure> {
        self.check_origin(reg.response.client_data_json.as_ref())?;
        let passkey = self
            .webauthn
            .finish_passkey_registration(reg, state)
            .map_err(refusal)?;
        // The unsigned credProps hint only after the signed data verified.
        if reg.extensions.cred_props.as_ref().and_then(|p| p.rk) == Some(false) {
            return Err(PasskeyFailure::NotDiscoverable);
        }
        let cred = webauthn_rs::prelude::Credential::from(passkey.clone());
        // UV is required by the registration policy; checked again here.
        if !cred.user_verified {
            return Err(PasskeyFailure::Invalid);
        }
        let transports = cred
            .transports
            .unwrap_or_default()
            .iter()
            .filter_map(|t| serde_json::to_value(t).ok())
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
        Ok(VerifiedRegistration {
            aaguid: aaguid_from_attestation_object(reg.response.attestation_object.as_ref()),
            backup_eligible: cred.backup_eligible,
            backup_state: cred.backup_state,
            transports,
            passkey,
        })
    }

    /// Discoverable request options (`allowCredentials: []`, UV required).
    /// `mediation` is left to the client.
    pub fn start_sign_in(
        &self,
    ) -> Result<(RequestChallengeResponse, DiscoverableAuthentication), PasskeyFailure> {
        let (mut rcr, state) = self
            .webauthn
            .start_discoverable_authentication()
            .map_err(internal)?;
        rcr.mediation = None;
        Ok((rcr, state))
    }

    /// The user handle and credential id of a discoverable assertion.
    pub fn identify(&self, cred: &PublicKeyCredential) -> Result<(Uuid, Vec<u8>), PasskeyFailure> {
        let (handle, id) = self
            .webauthn
            .identify_discoverable_authentication(cred)
            .map_err(|_| PasskeyFailure::UserHandleMissing)?;
        Ok((handle, id.to_vec()))
    }

    /// Verify a discoverable assertion against the stored credential, then
    /// UV and BE == `stored_be` (B.1).
    pub fn finish_sign_in(
        &self,
        cred: &PublicKeyCredential,
        state: DiscoverableAuthentication,
        stored: &Passkey,
        stored_be: bool,
    ) -> Result<AuthenticationResult, PasskeyFailure> {
        self.check_origin(cred.response.client_data_json.as_ref())?;
        let key = webauthn_rs::prelude::DiscoverableKey::from(&without_counter(stored));
        let result = self
            .webauthn
            .finish_discoverable_authentication(cred, state, &[key])
            .map_err(refusal)?;
        check_flags(&result, stored_be)?;
        Ok(result)
    }

    /// Request options over the caller's own passkeys (DC3).
    pub fn start_step_up(
        &self,
        passkeys: &[Passkey],
    ) -> Result<(RequestChallengeResponse, PasskeyAuthentication), PasskeyFailure> {
        let creds: Vec<Passkey> = passkeys.iter().map(without_counter).collect();
        self.webauthn
            .start_passkey_authentication(&creds)
            .map_err(internal)
    }

    /// Verify a step-up assertion; BE is compared here because the
    /// allow-list path of the crate accepts a BE upgrade.
    pub fn finish_step_up(
        &self,
        cred: &PublicKeyCredential,
        state: &PasskeyAuthentication,
        stored_be: bool,
    ) -> Result<AuthenticationResult, PasskeyFailure> {
        self.check_origin(cred.response.client_data_json.as_ref())?;
        let result = self
            .webauthn
            .finish_passkey_authentication(cred, state)
            .map_err(refusal)?;
        check_flags(&result, stored_be)?;
        Ok(result)
    }
}

/// The crate's working copy of a stored passkey, with the signature counter
/// set to 0.
///
/// B.1: "The signature counter is a signal, never an automatic lockout."
/// webauthn-rs 0.5.5 refuses a counter that did not grow and has no switch
/// for it (`require_valid_counter_value` is fixed to `true`). With a stored
/// counter of 0 the crate skips only that comparison; it still verifies the
/// signature over `authenticatorData` (which carries the received counter),
/// the challenge, origin, RP ID hash and UV. Kaiku compares the received
/// counter with the stored one itself ([`counter_regressed`]) and keeps the
/// stored high-water mark (`queries::record_use`). This is why
/// `danger-credential-internals` is enabled: it only exposes the credential
/// fields, the verification code paths are the crate's own.
fn without_counter(p: &Passkey) -> Passkey {
    let mut cred = webauthn_rs::prelude::Credential::from(p.clone());
    cred.counter = 0;
    Passkey::from(cred)
}

/// The stored signature counter of a passkey.
#[must_use]
pub fn stored_counter(p: &Passkey) -> u32 {
    webauthn_rs::prelude::Credential::from(p.clone()).counter
}

/// `WebAuthn` §6.1.1: a counter that did not grow while one was stored is a
/// cloning signal (audited, never a refusal).
#[must_use]
pub const fn counter_regressed(stored: u32, received: u32) -> bool {
    stored > 0 && received <= stored
}

fn internal(e: impl std::fmt::Display) -> PasskeyFailure {
    PasskeyFailure::Internal(e.to_string())
}

/// Crate refusals are `Invalid` (logged at debug, never returned to
/// clients). The crate's counter refusal cannot occur: it verifies a copy
/// with counter 0, and LP records a regression as a signal (B.1).
fn refusal(e: webauthn_rs::prelude::WebauthnError) -> PasskeyFailure {
    tracing::debug!(error = %e, "passkey verification refused");
    PasskeyFailure::Invalid
}

/// UV must be set; BE must equal the value stored at registration (§6.1.3).
pub fn check_flags(result: &AuthenticationResult, stored_be: bool) -> Result<(), PasskeyFailure> {
    if !result.user_verified() || result.backup_eligible() != stored_be {
        return Err(PasskeyFailure::Invalid);
    }
    Ok(())
}

/// The AAGUID from `authData` (AT flag `0x40` at byte 32, bytes 37..53).
/// All zeros or anything unparsable gives `None`.
#[must_use]
pub fn aaguid_from_attestation_object(att: &[u8]) -> Option<Uuid> {
    let value: serde_cbor_2::Value = serde_cbor_2::from_slice(att).ok()?;
    let serde_cbor_2::Value::Map(map) = value else {
        return None;
    };
    let auth_data = match map.get(&serde_cbor_2::Value::Text("authData".to_string()))? {
        serde_cbor_2::Value::Bytes(b) => b,
        _ => return None,
    };
    if auth_data.len() < 53 || auth_data[32] & 0x40 == 0 {
        return None;
    }
    let bytes: [u8; 16] = auth_data[37..53].try_into().ok()?;
    let id = Uuid::from_bytes(bytes);
    (!id.is_nil()).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crate_counter_refusal_is_plain_invalid() {
        // Review round 2 (Fable): the zero-counter copy makes the crate's
        // counter refusal unreachable; should it ever come, it is an
        // ordinary PASSKEY_INVALID, not a counter signal of its own.
        assert_eq!(
            refusal(webauthn_rs::prelude::WebauthnError::CredentialPossibleCompromise),
            PasskeyFailure::Invalid
        );
    }
    use crate::config::PublicOrigin;

    #[test]
    fn rp_allowed_origins_are_exact() {
        let public = PublicOrigin::parse("https://chat.example.com").unwrap();
        let config = PasskeyConfig::new(
            &public,
            None,
            Some("18:6F:F0:75:BF:FB:D4:76:5E:1C:5C:D9:E1:22:C4:09:44:58:A3:58:BC:E5:34:F5:3E:49:D7:B2:C3:81:AB:C0"),
        )
        .unwrap();
        let rp = PasskeyRp::new(config).unwrap();
        assert_eq!(rp.rp_id(), "chat.example.com");
        assert_eq!(
            rp.allowed_origins(),
            [
                "android:apk-key-hash:GG_wdb_71HZeHFzZ4SLECURYo1i85TT1PknXssOBq8A".to_string(),
                "https://chat.example.com".to_string(),
            ]
        );
        assert!(rp.allowed_origins().iter().all(|o| !o.contains("tauri")));
        let crate_origins: Vec<String> = rp
            .webauthn
            .get_allowed_origins()
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            crate_origins,
            [
                "https://chat.example.com/",
                "android:apk-key-hash:GG_wdb_71HZeHFzZ4SLECURYo1i85TT1PknXssOBq8A",
            ]
        );
    }
}
