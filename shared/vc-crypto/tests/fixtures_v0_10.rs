//! Data written by vodozemac 0.10.0 must stay readable by the vodozemac
//! version in the lockfile. The fixtures were generated once with the real
//! 0.10.0 crate (see the header of `fixtures/vodozemac_0_10.txt`) and must not
//! be regenerated with a newer version.

use std::collections::HashMap;

use vc_crypto::olm::{OlmAccount, OlmSession};
use vc_crypto::types::Curve25519PublicKey;

const FIXTURES: &str = include_str!("fixtures/vodozemac_0_10.txt");
const KEY: [u8; 32] = [0x42; 32];

fn fixtures() -> HashMap<&'static str, &'static str> {
    FIXTURES
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (k, v) = l.split_once(" = ").expect("fixture line `name = value`");
            (k.trim(), v.trim())
        })
        .collect()
}

fn get(name: &str) -> &'static str {
    fixtures()
        .get(name)
        .copied()
        .unwrap_or_else(|| panic!("missing fixture {name}"))
}

fn prekey(name: &str) -> vc_crypto::olm::EncryptedMessage {
    vc_crypto::olm::EncryptedMessage {
        message_type: 0,
        ciphertext: get(name).to_owned(),
    }
}

fn normal(name: &str) -> vc_crypto::olm::EncryptedMessage {
    vc_crypto::olm::EncryptedMessage {
        message_type: 1,
        ciphertext: get(name).to_owned(),
    }
}

#[test]
fn account_pickles_from_0_10_load() {
    let bob = OlmAccount::deserialize(get("bob_account_pickle"), &KEY).expect("bob account");
    let keys = bob.identity_keys();
    assert_eq!(keys.curve25519, get("bob_identity_curve25519"));
    assert_eq!(keys.ed25519, get("bob_identity_ed25519"));
    // Two unpublished one-time keys were stored in the pickle.
    assert_eq!(bob.one_time_keys().len(), 2);

    let alice = OlmAccount::deserialize(get("alice_account_pickle"), &KEY).expect("alice account");
    assert_eq!(
        alice.identity_keys().curve25519,
        get("alice_identity_curve25519")
    );
}

#[test]
fn prekey_message_from_0_10_opens_inbound_session_on_restored_account() {
    let mut bob = OlmAccount::deserialize(get("bob_account_pickle"), &KEY).expect("bob account");
    let alice_key = Curve25519PublicKey::from_base64(get("alice_identity_curve25519")).unwrap();
    let message = prekey("olm_prekey_message")
        .into_prekey_message()
        .expect("prekey message parses");

    let (mut session, plaintext) = bob
        .create_inbound_session(&alice_key, &message)
        .expect("inbound session from a 0.10 prekey message");
    assert_eq!(plaintext, "olm prekey message from alice");

    // The new session answers and the reply is a normal message.
    let reply = session.encrypt("reply from the current version").unwrap();
    assert!(!reply.is_prekey());
}

#[test]
fn olm_session_pickles_from_0_10_decrypt_and_continue() {
    let mut alice =
        OlmSession::deserialize(get("alice_session_pickle"), &KEY).expect("alice session");
    let mut bob = OlmSession::deserialize(get("bob_session_pickle"), &KEY).expect("bob session");
    assert_eq!(alice.session_id(), bob.session_id());

    // The stored normal message from Bob decrypts on Alice's restored session.
    assert_eq!(
        alice.decrypt(&normal("olm_normal_message")).unwrap(),
        "olm normal message from bob"
    );

    // Restored sessions keep talking to each other in both directions.
    for i in 0..3 {
        let text = format!("alice to bob {i}");
        let m = alice.encrypt(&text).unwrap();
        assert_eq!(bob.decrypt(&m).unwrap(), text);

        let text = format!("bob to alice {i}");
        let m = bob.encrypt(&text).unwrap();
        assert_eq!(alice.decrypt(&m).unwrap(), text);
    }
}

#[cfg(feature = "megolm")]
mod megolm {
    use vc_crypto::megolm::{MegolmInboundSession, MegolmOutboundSession};

    use super::{get, KEY};

    #[test]
    fn megolm_pickles_from_0_10_decrypt_and_continue() {
        let mut inbound =
            MegolmInboundSession::deserialize(get("megolm_inbound_pickle"), &KEY).expect("inbound");
        let mut outbound = MegolmOutboundSession::deserialize(get("megolm_outbound_pickle"), &KEY)
            .expect("outbound");

        assert_eq!(inbound.session_id(), outbound.session_id());
        assert_eq!(inbound.first_known_index(), 0);
        assert_eq!(outbound.message_index(), 6);

        // Messages from 0.10 at index 0 and at a later index (out of order).
        assert_eq!(
            inbound.decrypt(get("megolm_message_5")).unwrap(),
            "megolm message at index 5"
        );
        assert_eq!(
            inbound.decrypt(get("megolm_message_0")).unwrap(),
            "megolm message at index 0"
        );

        // The restored outbound session continues; its restored peer reads it.
        let ciphertext = outbound.encrypt("index 6 from the current version");
        assert_eq!(outbound.message_index(), 7);
        assert_eq!(
            inbound.decrypt(&ciphertext).unwrap(),
            "index 6 from the current version"
        );
    }

    #[test]
    fn new_inbound_session_from_0_10_session_key_reads_0_10_messages() {
        let mut inbound = MegolmInboundSession::new(get("megolm_session_key")).expect("key");
        assert_eq!(
            inbound.decrypt(get("megolm_message_0")).unwrap(),
            "megolm message at index 0"
        );
        assert_eq!(
            inbound.decrypt(get("megolm_message_5")).unwrap(),
            "megolm message at index 5"
        );
    }
}
