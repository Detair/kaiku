//! Push transports configured for this server instance.

use std::sync::Arc;

use super::fcm::FcmClient;
use crate::config::Config;

/// Holds the configured push transports. `UnifiedPush` needs no server-side
/// state; FCM is present only when a service account is configured.
#[derive(Debug, Default)]
pub struct PushService {
    fcm: Option<Arc<FcmClient>>,
}

impl PushService {
    /// Build from config. A key that cannot be used was already rejected by
    /// `Config::from_env`; if one slips through (hand-built config), FCM stays
    /// off and an error is logged rather than panicking.
    #[must_use]
    pub fn from_config(config: &Config) -> Self {
        let Some(key) = config.fcm_service_account.clone() else {
            tracing::info!("push: FCM disabled (no service account)");
            return Self { fcm: None };
        };
        let project_id = key.project_id.clone();
        match FcmClient::new(key, &config.fcm_api_base) {
            Ok(client) => {
                tracing::info!("push: FCM enabled for project {project_id}");
                Self {
                    fcm: Some(Arc::new(client)),
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "push: FCM disabled (service account unusable)");
                Self { fcm: None }
            }
        }
    }

    /// The FCM client, when FCM is configured.
    #[must_use]
    pub fn fcm(&self) -> Option<&FcmClient> {
        self.fcm.as_deref()
    }
}
