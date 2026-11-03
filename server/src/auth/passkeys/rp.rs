//! `webauthn-rs` in Kaiku's passkey profile (B.1, C29).

use super::config::PasskeyConfig;

#[derive(Debug, Clone)]
pub struct PasskeyRp {
    config: PasskeyConfig,
    #[allow(dead_code)] // used by the profile operations (Task 3)
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
}

#[cfg(test)]
mod tests {
    use super::*;
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
