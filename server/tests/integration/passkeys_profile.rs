//! WebAuthn profile (C29 "WebAuthn JSON", B.1 "Passkeys") and the
//! single-use ceremony store (LP Task 3).

use fred::interfaces::KeysInterface;
use serde_json::{json, Value};
use uuid::Uuid;
use vc_server::auth::passkeys::ceremony::{self, CeremonyKind, CeremonyRecord, CeremonyState};
use vc_server::auth::passkeys::config::apk_key_hash_origin;
use vc_server::auth::passkeys::rp::{
    aaguid_from_attestation_object, PasskeyFailure, PasskeyRp, VerifiedRegistration,
};
use webauthn_rs::prelude::{PublicKeyCredential, RegisterPublicKeyCredential};

use crate::helpers::login::TEST_ORIGIN;
use crate::helpers::passkeys::{test_passkey_rp, SoftAuthenticator, TEST_APK_ORIGIN};

fn b64u_decode(s: &str) -> Vec<u8> {
    vc_server::approvals::secrets::b64u_decode(s).expect("b64u")
}

/// Register a passkey for a fresh user handle from the web origin.
fn register(rp: &PasskeyRp, auth: &mut SoftAuthenticator) -> (Uuid, VerifiedRegistration) {
    let handle = Uuid::new_v4();
    let (ccr, state) = rp
        .start_registration(handle, "alice", "Alice", vec![])
        .expect("start registration");
    let options = serde_json::to_value(&ccr).unwrap();
    let cred = auth.register(TEST_ORIGIN, &options);
    let reg: RegisterPublicKeyCredential = serde_json::from_value(cred).unwrap();
    (
        handle,
        rp.finish_registration(&reg, &state).expect("finish"),
    )
}

fn cred_id(v: &VerifiedRegistration) -> Vec<u8> {
    v.passkey.cred_id().as_ref().to_vec()
}

/// A discoverable sign-in from `origin`.
fn sign_in(
    rp: &PasskeyRp,
    auth: &mut SoftAuthenticator,
    handle: Uuid,
    v: &VerifiedRegistration,
    origin: &str,
    uv: bool,
    stored_be: bool,
) -> Result<(), PasskeyFailure> {
    let (rcr, state) = rp.start_sign_in().expect("start sign-in");
    let options = serde_json::to_value(&rcr).unwrap();
    let a = auth.assert(origin, &options, &cred_id(v), Some(handle), uv);
    let cred: PublicKeyCredential = serde_json::from_value(a).unwrap();
    let (h, id) = rp.identify(&cred)?;
    assert_eq!((h, id), (handle, cred_id(v)));
    rp.finish_sign_in(&cred, state, &v.passkey, stored_be)
        .map(|_| ())
}

fn step_up(
    rp: &PasskeyRp,
    auth: &mut SoftAuthenticator,
    handle: Uuid,
    v: &VerifiedRegistration,
    stored_be: bool,
) -> Result<(), PasskeyFailure> {
    let (rcr, state) = rp.start_step_up(std::slice::from_ref(&v.passkey)).unwrap();
    let options = serde_json::to_value(&rcr).unwrap();
    let a = auth.assert(TEST_ORIGIN, &options, &cred_id(v), Some(handle), true);
    let cred: PublicKeyCredential = serde_json::from_value(a).unwrap();
    rp.finish_step_up(&cred, &state, stored_be).map(|_| ())
}

#[test]
fn registration_options_follow_the_profile() {
    let rp = test_passkey_rp();
    let exclude = vec![vec![1u8; 16].into(), vec![2u8; 16].into()];
    let (ccr, _) = rp
        .start_registration(Uuid::new_v4(), "alice", "Alice", exclude)
        .unwrap();
    let o = serde_json::to_value(&ccr).unwrap();
    let pk = &o["publicKey"];
    assert_eq!(
        pk["authenticatorSelection"],
        json!({"residentKey": "required", "requireResidentKey": true, "userVerification": "required"})
    );
    assert_eq!(pk["attestation"], "none");
    let excluded: Vec<Vec<u8>> = pk["excludeCredentials"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| b64u_decode(c["id"].as_str().unwrap()))
        .collect();
    assert_eq!(excluded, vec![vec![1u8; 16], vec![2u8; 16]]);
    assert!(b64u_decode(pk["challenge"].as_str().unwrap()).len() >= 16);
    assert_eq!(pk["rp"]["id"], "chat.example.com");
    assert_eq!(pk["extensions"]["credProps"], true);
}

#[test]
fn sign_in_options_have_empty_allow_credentials() {
    let rp = test_passkey_rp();
    let (rcr, _) = rp.start_sign_in().unwrap();
    let o = serde_json::to_value(&rcr).unwrap();
    assert_eq!(o["publicKey"]["allowCredentials"], json!([]));
    assert_eq!(o["publicKey"]["userVerification"], "required");
    assert!(o.get("mediation").is_none_or(Value::is_null), "{o}");
    assert!(b64u_decode(o["publicKey"]["challenge"].as_str().unwrap()).len() >= 16);
}

#[test]
fn assertion_from_web_origin_is_accepted() {
    let rp = test_passkey_rp();
    let mut auth = SoftAuthenticator::new();
    let (h, v) = register(&rp, &mut auth);
    sign_in(&rp, &mut auth, h, &v, TEST_ORIGIN, true, v.backup_eligible).unwrap();
    step_up(&rp, &mut auth, h, &v, v.backup_eligible).unwrap();
}

#[test]
fn assertion_from_configured_apk_origin_is_accepted() {
    let rp = test_passkey_rp();
    let mut auth = SoftAuthenticator::new();
    let (h, v) = register(&rp, &mut auth);
    sign_in(
        &rp,
        &mut auth,
        h,
        &v,
        TEST_APK_ORIGIN,
        true,
        v.backup_eligible,
    )
    .unwrap();
}

fn refused_from(origin: &str) {
    let rp = test_passkey_rp();
    let mut auth = SoftAuthenticator::new();
    let (h, v) = register(&rp, &mut auth);
    assert_eq!(
        sign_in(&rp, &mut auth, h, &v, origin, true, v.backup_eligible),
        Err(PasskeyFailure::Invalid),
        "{origin}"
    );
}

#[test]
fn assertion_from_other_apk_hash_is_refused() {
    let mut seq = [0u8; 32];
    for (i, b) in seq.iter_mut().enumerate() {
        *b = u8::try_from(i).unwrap();
    }
    refused_from(&apk_key_hash_origin(&seq));
}

#[test]
fn padded_apk_hash_is_refused() {
    refused_from(&format!("{TEST_APK_ORIGIN}="));
}

#[test]
fn case_changed_apk_hash_is_refused() {
    let (prefix, hash) = TEST_APK_ORIGIN.split_at("android:apk-key-hash:".len());
    refused_from(&format!("{prefix}{}", hash.to_lowercase()));
    refused_from(&format!("{prefix}{}", hash.to_uppercase()));
    refused_from(&TEST_APK_ORIGIN.replacen("android", "ANDROID", 1));
    refused_from("HTTPS://chat.example.com");
    refused_from("https://chat.example.com/");
}

#[test]
fn parent_domain_origin_is_refused() {
    refused_from("https://example.com");
    refused_from("https://evil.chat.example.com");
    refused_from("https://chat.example.com:8443");
}

#[test]
fn tauri_origin_is_refused() {
    refused_from("http://tauri.localhost");
    refused_from("tauri://localhost");
}

#[test]
fn assertion_without_user_verification_is_refused() {
    let rp = test_passkey_rp();
    let mut auth = SoftAuthenticator::new();
    let (h, v) = register(&rp, &mut auth);
    assert_eq!(
        sign_in(&rp, &mut auth, h, &v, TEST_ORIGIN, false, v.backup_eligible),
        Err(PasskeyFailure::Invalid)
    );
}

#[test]
fn backup_eligibility_change_is_refused() {
    let rp = test_passkey_rp();
    let mut auth = SoftAuthenticator::new();
    let (h, v) = register(&rp, &mut auth);
    // The soft token never sets BE; the stored value says it did.
    assert!(!v.backup_eligible);
    assert_eq!(
        sign_in(&rp, &mut auth, h, &v, TEST_ORIGIN, true, true),
        Err(PasskeyFailure::Invalid)
    );
    assert_eq!(
        step_up(&rp, &mut auth, h, &v, true),
        Err(PasskeyFailure::Invalid)
    );
}

#[test]
fn missing_user_handle_is_refused() {
    let rp = test_passkey_rp();
    let mut auth = SoftAuthenticator::new();
    let (_, v) = register(&rp, &mut auth);
    let (rcr, _) = rp.start_sign_in().unwrap();
    let options = serde_json::to_value(&rcr).unwrap();
    let a = auth.assert(TEST_ORIGIN, &options, &cred_id(&v), None, true);
    let cred: PublicKeyCredential = serde_json::from_value(a).unwrap();
    assert_eq!(rp.identify(&cred), Err(PasskeyFailure::UserHandleMissing));
}

#[test]
fn credprops_rk_false_is_refused() {
    let rp = test_passkey_rp();
    let mut auth = SoftAuthenticator::new();
    let (ccr, state) = rp
        .start_registration(Uuid::new_v4(), "alice", "Alice", vec![])
        .unwrap();
    let mut cred = auth.register(TEST_ORIGIN, &serde_json::to_value(&ccr).unwrap());
    // Browsers send `clientExtensionResults` (toJSON()).
    cred.as_object_mut().unwrap().remove("extensions");
    cred["clientExtensionResults"] = json!({"credProps": {"rk": false}});
    let reg: RegisterPublicKeyCredential = serde_json::from_value(cred.clone()).unwrap();
    assert_eq!(
        rp.finish_registration(&reg, &state).unwrap_err(),
        PasskeyFailure::NotDiscoverable
    );
    // `rk: true` and a missing credProps are accepted (DC9).
    cred["clientExtensionResults"] = json!({"credProps": {"rk": true}});
    let reg: RegisterPublicKeyCredential = serde_json::from_value(cred).unwrap();
    rp.finish_registration(&reg, &state).unwrap();
}

/// A CBOR attestation object `{fmt: "none", attStmt: {}, authData}`.
fn attestation_object(auth_data: &[u8]) -> Vec<u8> {
    let mut out = vec![0xa3];
    out.push(0x63);
    out.extend_from_slice(b"fmt");
    out.push(0x64);
    out.extend_from_slice(b"none");
    out.push(0x67);
    out.extend_from_slice(b"attStmt");
    out.push(0xa0);
    out.push(0x68);
    out.extend_from_slice(b"authData");
    out.push(0x59);
    out.extend_from_slice(&u16::try_from(auth_data.len()).unwrap().to_be_bytes());
    out.extend_from_slice(auth_data);
    out
}

fn auth_data(flags: u8, aaguid: [u8; 16]) -> Vec<u8> {
    let mut d = vec![0u8; 32];
    d.push(flags);
    d.extend_from_slice(&[0, 0, 0, 1]);
    d.extend_from_slice(&aaguid);
    d.extend_from_slice(&[0, 2, 9, 9]);
    d
}

#[test]
fn aaguid_is_read_from_auth_data() {
    let aaguid = Uuid::parse_str("ea9b8d66-4d01-1d21-3ce4-b6b48cb575d4").unwrap();
    let att = attestation_object(&auth_data(0x45, *aaguid.as_bytes()));
    assert_eq!(aaguid_from_attestation_object(&att), Some(aaguid));
    // All zeros, no AT flag, garbage → None.
    let zeros = attestation_object(&auth_data(0x45, [0; 16]));
    assert_eq!(aaguid_from_attestation_object(&zeros), None);
    let no_at = attestation_object(&auth_data(0x05, *aaguid.as_bytes()));
    assert_eq!(aaguid_from_attestation_object(&no_at), None);
    assert_eq!(aaguid_from_attestation_object(b"\xff\x00garbage"), None);
    assert_eq!(
        aaguid_from_attestation_object(&attestation_object(&[0; 40])),
        None
    );
}

async fn redis() -> fred::clients::Client {
    let config = vc_server::config::Config::default_for_test();
    vc_server::db::create_redis_client(&config.redis_url)
        .await
        .expect("Valkey")
}

fn record() -> CeremonyRecord {
    let (_, state) = test_passkey_rp().start_sign_in().unwrap();
    CeremonyRecord {
        kind: CeremonyKind::SignIn { flow_id: None },
        state: CeremonyState::SignIn(state),
        created_at: chrono::Utc::now().timestamp(),
    }
}

#[tokio::test]
async fn ceremony_is_single_use() {
    let redis = redis().await;
    let id = ceremony::new_ceremony_id();
    ceremony::put_new(&redis, id, &record()).await.unwrap();
    // NX: the same id cannot be stored twice.
    assert!(ceremony::put_new(&redis, id, &record()).await.is_err());
    assert!(ceremony::take(&redis, id).await.unwrap().is_some());
    assert!(ceremony::take(&redis, id).await.unwrap().is_none());
}

#[tokio::test]
async fn ceremony_ttl_is_300s() {
    let redis = redis().await;
    let id = ceremony::new_ceremony_id();
    ceremony::put_new(&redis, id, &record()).await.unwrap();
    let ttl: i64 = redis.ttl(ceremony::key(id)).await.unwrap();
    assert!(ttl > 0 && ttl <= ceremony::CEREMONY_TTL_SECS, "{ttl}");
    assert_eq!(ceremony::CEREMONY_TTL_SECS, 300);
    ceremony::take(&redis, id).await.unwrap();
}

#[test]
fn ceremony_ids_are_random_uuids() {
    let a = ceremony::new_ceremony_id();
    let b = ceremony::new_ceremony_id();
    assert_eq!(a.get_version_num(), 4);
    assert_ne!(a, b);
}
