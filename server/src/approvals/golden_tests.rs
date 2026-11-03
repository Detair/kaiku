//! C26 golden vectors (normative). The server and the Android tests read the
//! same file: `mobile/android/app/src/test/resources/golden/kaiku_sig_v1.json`.
//!
//! `KAIKU_UPDATE_GOLDEN=1` rewrites the computed fields (`c_hex`,
//! `context_hash_hex`, `signing_prefix_b64u`, `message_hex`) only; the
//! inputs and the signatures are copied from C26 and never regenerated.

use std::path::PathBuf;

use p256::ecdsa::Signature;
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::context::{CanonicalContext, LoginContext, Variant};
use super::secrets::{b64u_decode, b64u_encode};
use super::signing::{
    context_hash, message, parse_public_key, signing_prefix, verify, Decision, Kind,
    SignatureError, SigningInput,
};
use crate::auth::client_class::ClientClass;

const UPDATE_ENV: &str = "KAIKU_UPDATE_GOLDEN";

fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../mobile/android/app/src/test/resources/golden/kaiku_sig_v1.json")
}

fn load() -> Value {
    let raw = std::fs::read_to_string(golden_path()).expect("golden file readable");
    serde_json::from_str(&raw).expect("golden file is JSON")
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or_else(|| panic!("{k} is a string"))
}

fn kind_of(v: &Value) -> Kind {
    match s(v, "kind") {
        "login" => Kind::Login,
        "enroll" => Kind::Enroll,
        "device_login" => Kind::DeviceLogin,
        "device_stepup" => Kind::DeviceStepup,
        "enroll_pop" => Kind::EnrollPop,
        other => panic!("unknown kind {other}"),
    }
}

fn decision_of(v: &Value) -> Decision {
    match s(v, "decision") {
        "approve" => Decision::Approve,
        "report" => Decision::Report,
        other => panic!("unknown decision {other}"),
    }
}

/// Everything the vector's inputs determine.
struct Computed {
    c: Vec<u8>,
    hash: [u8; 32],
    prefix: Vec<u8>,
    message: Vec<u8>,
}

fn compute(v: &Value) -> Computed {
    let kind = kind_of(v);
    let ctx = CanonicalContext::from_json(kind, &v["context"]).expect("context parses");
    assert_eq!(ctx.kind(), kind);
    let c = ctx.encode();
    let hash = ctx.hash();
    assert_eq!(hash, context_hash(kind, &c));
    let nonce: [u8; 32] = hex::decode(s(v, "nonce_hex")).unwrap().try_into().unwrap();
    let input = SigningInput {
        kind,
        challenge_id: Uuid::parse_str(s(v, "challenge_id")).unwrap(),
        nonce,
        expires_at: v["expires_at"].as_i64().unwrap(),
        origin: s(v, "origin"),
        user_id: Uuid::parse_str(s(v, "user_id")).unwrap(),
        device_id: Uuid::parse_str(s(v, "device_id")).unwrap(),
        context_hash: hash,
    };
    let prefix = signing_prefix(&input);
    let response = hex::decode(s(v, "response_hex")).unwrap();
    let message = message(&prefix, decision_of(v), &response);
    Computed {
        c,
        hash,
        prefix,
        message,
    }
}

fn vectors(g: &Value) -> &Vec<Value> {
    g["vectors"].as_array().expect("vectors array")
}

fn vector<'a>(g: &'a Value, name: &str) -> &'a Value {
    vectors(g)
        .iter()
        .find(|v| s(v, "name") == name)
        .unwrap_or_else(|| panic!("{name} present"))
}

/// Rewrites the computed fields when `KAIKU_UPDATE_GOLDEN=1`; a no-op
/// otherwise. Runs first in every test that reads computed fields.
fn maybe_update() {
    if std::env::var(UPDATE_ENV).as_deref() != Ok("1") {
        return;
    }
    let mut g = load();
    for v in g["vectors"].as_array_mut().unwrap() {
        let c = compute(v);
        v["c_hex"] = Value::String(hex::encode(&c.c));
        v["context_hash_hex"] = Value::String(hex::encode(c.hash));
        v["signing_prefix_b64u"] = Value::String(b64u_encode(&c.prefix));
        v["message_hex"] = Value::String(hex::encode(&c.message));
    }
    let mut out = serde_json::to_string_pretty(&g).unwrap();
    out.push('\n');
    std::fs::write(golden_path(), out).unwrap();
}

#[test]
fn golden_file_has_v01_to_v05() {
    let g = load();
    assert_eq!(g["version"], 1);
    let names: Vec<&str> = vectors(&g).iter().map(|v| s(v, "name")).collect();
    assert_eq!(names, ["V01", "V02", "V03", "V04", "V05"]);
    let kinds: Vec<&str> = vectors(&g).iter().map(|v| s(v, "kind")).collect();
    assert_eq!(
        kinds,
        [
            "login",
            "enroll",
            "device_login",
            "device_stepup",
            "enroll_pop"
        ]
    );
}

#[test]
fn context_encoding_matches_c26() {
    maybe_update();
    let g = load();
    for v in vectors(&g) {
        let c = compute(v);
        assert_eq!(hex::encode(&c.c), s(v, "c_hex"), "{}", s(v, "name"));
        assert_eq!(hex::encode(c.hash), s(v, "context_hash_hex"));
        assert_eq!(b64u_encode(&c.prefix), s(v, "signing_prefix_b64u"));
        assert_eq!(hex::encode(&c.message), s(v, "message_hex"));
    }
    // C26's printed C for V01 and V03.
    assert_eq!(
        hex::encode(compute(vector(&g, "V01")).c),
        "4444444444444444844444444444444413d31e961a1ad8ec2f16b10c4c982e0876a878ad6df144566ee1894acb70f9c3\
         0102000000184b61696b75206465736b746f70206f6e2057696e646f7773000000006ae9ccc00000000f46616d696c69\
         65204dc3bc6c6c65720000000b48616d627572672c20444500000005616c696365"
    );
    assert_eq!(hex::encode(compute(vector(&g, "V03")).c), "03");
}

#[test]
fn context_hashes_match_c26() {
    let g = load();
    for (name, want) in [
        (
            "V01",
            "2e0d6af64a671a1cc0a3cf4435231ffe08b56f6c3dbe9b74791da593e017fadf",
        ),
        (
            "V02",
            "95cc384c6457731aa857a59bceeb1709add698e15c1cdce489bc8c2eeaed3c1b",
        ),
        (
            "V03",
            "7bc190cbec9880136fdb564f293739d948deb11aeeb8dcaf95aa92c0424e29ae",
        ),
        (
            "V04",
            "daa72edfe0362bf796d8c7ee30267354b9491e0ec97e36087db40e8d362f61c9",
        ),
        (
            "V05",
            "0849d9cc3c467e1e5652673387a8847ba2a2803531cd9734256c5f58758122fc",
        ),
    ] {
        assert_eq!(hex::encode(compute(vector(&g, name)).hash), want, "{name}");
    }
}

#[test]
fn messages_match_c26() {
    let g = load();
    let v01 = compute(vector(&g, "V01")).message;
    assert_eq!(v01.len(), 198);
    assert_eq!(
        hex::encode(&v01),
        "6b61696b752d7369672d76310133333333333343338333333333333301000102030405060708090a0b0c0d0e0f101112\
         131415161718191a1b1c1d1e1f000000006ae9cd380000001868747470733a2f2f636861742e6578616d706c652e636f\
         6d11111111111141118111111111111111222222222222422282222222222222222e0d6af64a671a1cc0a3cf4435231f\
         fe08b56f6c3dbe9b74791da593e017fadf0100000020a0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9\
         babbbcbdbebf"
    );
    for (name, want, len) in [
        (
            "V02",
            "754b25e37dfac6fb063a3cc1c5b6238065ae171f0aae1da41aee6c3de74bd55c",
            172,
        ),
        (
            "V03",
            "a5cbee81abacb5b317edacf06d75159bf80333278d65636c2f784ed8f0592703",
            166,
        ),
        (
            "V04",
            "f76b957ca40e9625cc10ee1cb54d6e38d50380f1af778b9e24efa6a94ae0ebf7",
            166,
        ),
        (
            "V05",
            "7afd5d3b92e1a6f0f498df0cb963837c2f3f4231b55d590baf640921e85f8ad4",
            166,
        ),
    ] {
        let m = compute(vector(&g, name)).message;
        assert_eq!(m.len(), len, "{name}");
        assert_eq!(hex::encode(Sha256::digest(&m)), want, "{name}");
    }
}

#[test]
fn prefix_layout_matches_c26() {
    let g = load();
    for v in vectors(&g) {
        let c = compute(v);
        let p = &c.prefix;
        assert_eq!(&p[0..12], b"kaiku-sig-v1");
        assert_eq!(p[12], kind_of(v) as u8);
        assert_eq!(
            &p[13..29],
            Uuid::parse_str(s(v, "challenge_id")).unwrap().as_bytes()
        );
        assert_eq!(hex::encode(&p[29..61]), s(v, "nonce_hex"));
        assert_eq!(
            i64::from_be_bytes(p[61..69].try_into().unwrap()),
            v["expires_at"].as_i64().unwrap()
        );
        let origin = s(v, "origin").as_bytes();
        let olen = u32::from_be_bytes(p[69..73].try_into().unwrap()) as usize;
        assert_eq!(olen, origin.len());
        assert_eq!(&p[73..73 + olen], origin);
        let rest = &p[73 + olen..];
        assert_eq!(rest.len(), 64);
        assert_eq!(
            &rest[0..16],
            Uuid::parse_str(s(v, "user_id")).unwrap().as_bytes()
        );
        assert_eq!(
            &rest[16..32],
            Uuid::parse_str(s(v, "device_id")).unwrap().as_bytes()
        );
        assert_eq!(&rest[32..64], &c.hash);
        // prefix ‖ decision ‖ lp(response) == M
        let response = hex::decode(s(v, "response_hex")).unwrap();
        let mut m = p.clone();
        m.push(decision_of(v) as u8);
        m.extend_from_slice(&u32::try_from(response.len()).unwrap().to_be_bytes());
        m.extend_from_slice(&response);
        assert_eq!(m, c.message);
    }
}

fn golden_key(g: &Value) -> p256::ecdsa::VerifyingKey {
    parse_public_key(&b64u_decode(s(g, "public_key")).unwrap()).expect("golden key parses")
}

#[test]
fn golden_signatures_verify() {
    let g = load();
    let key = golden_key(&g);
    let mut high_s = Vec::new();
    for v in vectors(&g) {
        let sig = b64u_decode(s(v, "signature")).unwrap();
        let m = compute(v).message;
        verify(&key, &m, &sig).unwrap_or_else(|e| panic!("{}: {e:?}", s(v, "name")));
        let parsed = Signature::from_der(&sig).unwrap();
        if parsed.normalize_s().is_some() {
            high_s.push(s(v, "name").to_string());
        }
    }
    assert_eq!(high_s, ["V01", "V02", "V04"]);
}

#[test]
fn low_s_form_also_verifies() {
    let g = load();
    let key = golden_key(&g);
    for name in ["V01", "V02", "V04"] {
        let v = vector(&g, name);
        let sig = Signature::from_der(&b64u_decode(s(v, "signature")).unwrap()).unwrap();
        let low = sig.normalize_s().expect("high s");
        let m = compute(v).message;
        verify(&key, &m, low.to_der().as_bytes()).expect("low s verifies");
    }
}

#[test]
fn tampered_message_fails() {
    let g = load();
    let key = golden_key(&g);
    let v = vector(&g, "V01");
    let sig = b64u_decode(s(v, "signature")).unwrap();
    let m = compute(v).message;
    let decision_at = m.len() - 4 - 32 - 1;
    for idx in [decision_at - 1, decision_at, m.len() - 1] {
        let mut t = m.clone();
        t[idx] ^= 0x01;
        assert!(
            matches!(verify(&key, &t, &sig), Err(SignatureError::Invalid)),
            "byte {idx}"
        );
    }
    assert!(matches!(
        verify(&key, &m, b"not der"),
        Err(SignatureError::BadSignature)
    ));
}

#[test]
fn compressed_or_other_curve_keys_refused() {
    let g = load();
    let der = b64u_decode(s(&g, "public_key")).unwrap();
    let key = golden_key(&g);
    // Compressed point under the same P-256 SPKI header.
    let point = key.to_encoded_point(true);
    let mut compressed =
        hex::decode("3039301306072a8648ce3d020106082a8648ce3d030107032200").unwrap();
    compressed.extend_from_slice(point.as_bytes());
    assert!(matches!(
        parse_public_key(&compressed),
        Err(SignatureError::BadKey)
    ));
    // A P-384 SPKI (secp384r1 parameters).
    let mut p384 = hex::decode("3076301006072a8648ce3d020106052b81040022036200").unwrap();
    p384.push(0x04);
    p384.extend_from_slice(&[0x11; 96]);
    assert!(matches!(
        parse_public_key(&p384),
        Err(SignatureError::BadKey)
    ));
    // Truncated and trailing-garbage DER.
    assert!(matches!(
        parse_public_key(&der[..der.len() - 1]),
        Err(SignatureError::BadKey)
    ));
    let mut trailing = der.clone();
    trailing.push(0);
    assert!(matches!(
        parse_public_key(&trailing),
        Err(SignatureError::BadKey)
    ));
}

#[test]
fn unknown_kind_byte_refused() {
    for b in [0x00u8, 0x06, 0x10, 0x1f, 0xff] {
        assert!(Kind::from_byte(b).is_none(), "{b:#x}");
    }
    for (b, k) in [
        (1u8, Kind::Login),
        (2, Kind::Enroll),
        (3, Kind::DeviceLogin),
        (4, Kind::DeviceStepup),
        (5, Kind::EnrollPop),
    ] {
        assert_eq!(Kind::from_byte(b), Some(k));
    }
    assert!(Kind::Login.is_approval() && Kind::Enroll.is_approval());
    assert!(!Kind::DeviceLogin.is_approval() && !Kind::EnrollPop.is_approval());
}

fn login_ctx(server_name: &str, client_label: &str) -> CanonicalContext {
    CanonicalContext::Login(LoginContext {
        attempt_id: Uuid::nil(),
        collection_hash: [7; 32],
        variant: Variant::Qr,
        client_class: ClientClass::Web,
        client_label: client_label.to_string(),
        created_at: 1_793_707_200,
        server_name: server_name.to_string(),
        place: String::new(),
        username: "oma".to_string(),
    })
}

#[test]
fn strings_are_hashed_exactly_as_sent() {
    let nfc = "Familie M\u{00fc}ller";
    let nfd = "Familie Mu\u{0308}ller";
    let trailing = "Familie M\u{00fc}ller ";
    let contexts = [
        login_ctx(nfc, "Firefox on Linux"),
        login_ctx(nfd, "Firefox on Linux"),
        login_ctx(trailing, "Firefox on Linux"),
        login_ctx(nfc, "Opas Handy \u{1f4f1}"),
    ];
    let hashes: std::collections::HashSet<[u8; 32]> =
        contexts.iter().map(CanonicalContext::hash).collect();
    assert_eq!(hashes.len(), 4, "four distinct hashes");
    for ctx in &contexts {
        let json = ctx.to_json();
        // The JSON carries the exact bytes (serde_json neither normalises
        // nor trims), and parses back to the identical encoding.
        let wire = serde_json::to_string(&json).unwrap();
        let reparsed: Value = serde_json::from_str(&wire).unwrap();
        let back = CanonicalContext::from_json(Kind::Login, &reparsed).unwrap();
        assert_eq!(&back, ctx);
        assert_eq!(back.encode(), ctx.encode());
    }
    let CanonicalContext::Login(l) = &contexts[1] else {
        unreachable!()
    };
    assert_eq!(l.server_name.as_bytes(), nfd.as_bytes());
}

#[test]
fn from_json_refuses_wrong_shapes() {
    let g = load();
    let v = vector(&g, "V01");
    let mut ctx = v["context"].clone();
    assert!(CanonicalContext::from_json(Kind::Enroll, &ctx).is_err());
    ctx["variant"] = Value::String("sms".into());
    assert!(CanonicalContext::from_json(Kind::Login, &ctx).is_err());
    let mut ctx = v["context"].clone();
    ctx["collection_hash"] = Value::String("AAAA".into());
    assert!(CanonicalContext::from_json(Kind::Login, &ctx).is_err());
    let mut ctx = v["context"].clone();
    ctx.as_object_mut().unwrap().remove("username");
    assert!(CanonicalContext::from_json(Kind::Login, &ctx).is_err());
}
