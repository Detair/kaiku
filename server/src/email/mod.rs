//! Email Service
//!
//! SMTP-based email delivery for transactional emails (password resets, etc.).

use anyhow::{Context, Result};
use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::config::Config;

/// Email service for sending transactional emails via SMTP.
#[derive(Clone)]
pub struct EmailService {
    mailer: AsyncSmtpTransport<Tokio1Executor>,
    from_address: Mailbox,
}

impl EmailService {
    /// Create a new email service from server configuration.
    ///
    /// Requires SMTP to be fully configured (`config.has_smtp()` must be true).
    pub fn new(config: &Config) -> Result<Self> {
        let host = config.smtp_host.as_ref().context("SMTP_HOST is required")?;
        let username = config
            .smtp_username
            .as_ref()
            .context("SMTP_USERNAME is required")?;
        let password = config
            .smtp_password
            .as_ref()
            .context("SMTP_PASSWORD is required")?;
        let from = config.smtp_from.as_ref().context("SMTP_FROM is required")?;

        let from_address: Mailbox = from
            .parse()
            .context("SMTP_FROM is not a valid email address")?;

        let creds = Credentials::new(username.clone(), password.clone());

        let mailer = match config.smtp_tls.as_str() {
            "tls" => AsyncSmtpTransport::<Tokio1Executor>::relay(host)
                .context("Failed to create SMTP TLS transport")?
                .port(config.smtp_port)
                .credentials(creds)
                .build(),
            "none" => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host)
                .port(config.smtp_port)
                .credentials(creds)
                .build(),
            // Default: STARTTLS
            _ => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)
                .context("Failed to create SMTP STARTTLS transport")?
                .port(config.smtp_port)
                .credentials(creds)
                .build(),
        };

        Ok(Self {
            mailer,
            from_address,
        })
    }

    /// Test the SMTP connection by sending a NOOP command.
    pub async fn test_connection(&self) -> Result<()> {
        let ok = self
            .mailer
            .test_connection()
            .await
            .context("SMTP connection test failed")?;
        if !ok {
            anyhow::bail!("SMTP server did not respond positively to connection test");
        }
        Ok(())
    }

    /// Send a password reset email with the given reset code.
    pub async fn send_password_reset(
        &self,
        to_email: &str,
        username: &str,
        reset_token: &str,
    ) -> Result<()> {
        let to_mailbox: Mailbox = to_email
            .parse()
            .context("Invalid recipient email address")?;

        let body = format!(
            "Hello {username},\n\
             \n\
             A password reset was requested for your account.\n\
             \n\
             Your reset code: {reset_token}\n\
             \n\
             Enter this code on the password reset page to set a new password.\n\
             This code expires in 1 hour.\n\
             \n\
             If you did not request this, you can safely ignore this email.\n"
        );

        let email = Message::builder()
            .from(self.from_address.clone())
            .to(to_mailbox)
            .subject("Password Reset Request")
            .body(body)
            .context("Failed to build email message")?;

        self.mailer
            .send(email)
            .await
            .context("Failed to send email via SMTP")?;

        Ok(())
    }

    /// Send a notification that the user's data export is ready for download.
    pub async fn send_data_export_ready(
        &self,
        to_email: &str,
        username: &str,
    ) -> Result<()> {
        let to_mailbox: Mailbox = to_email
            .parse()
            .context("Invalid recipient email address")?;

        let body = format!(
            "Hello {username},\n\
             \n\
             Your data export is ready for download.\n\
             \n\
             You can download it from your account settings.\n\
             \n\
             The download link will expire in 7 days.\n"
        );

        let email = Message::builder()
            .from(self.from_address.clone())
            .to(to_mailbox)
            .subject("Your Data Export is Ready")
            .body(body)
            .context("Failed to build email message")?;

        self.mailer
            .send(email)
            .await
            .context("Failed to send export notification email")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a Config with all SMTP fields populated (using `smtp_tls: "none"`
    /// to avoid DNS resolution / TLS handshake in tests).
    fn smtp_test_config() -> Config {
        let mut config = Config::default_for_test();
        config.smtp_host = Some("localhost".into());
        config.smtp_username = Some("testuser".into());
        config.smtp_password = Some("testpass".into());
        config.smtp_from = Some("noreply@example.com".into());
        config.smtp_tls = "none".into();
        config
    }

    /// Extract the error from a Result<EmailService>, panicking if Ok.
    fn expect_err(result: Result<EmailService>) -> anyhow::Error {
        match result {
            Err(e) => e,
            Ok(_) => panic!("Expected error, got Ok"),
        }
    }

    #[test]
    fn test_new_success() {
        let config = smtp_test_config();
        let result = EmailService::new(&config);
        assert!(
            result.is_ok(),
            "EmailService::new should succeed with valid SMTP config"
        );
    }

    #[test]
    fn test_new_missing_host() {
        let mut config = smtp_test_config();
        config.smtp_host = None;
        let err = expect_err(EmailService::new(&config));
        assert!(
            err.to_string().contains("SMTP_HOST"),
            "Error should mention SMTP_HOST: {err}"
        );
    }

    #[test]
    fn test_new_missing_username() {
        let mut config = smtp_test_config();
        config.smtp_username = None;
        let err = expect_err(EmailService::new(&config));
        assert!(
            err.to_string().contains("SMTP_USERNAME"),
            "Error should mention SMTP_USERNAME: {err}"
        );
    }

    #[test]
    fn test_new_missing_password() {
        let mut config = smtp_test_config();
        config.smtp_password = None;
        let err = expect_err(EmailService::new(&config));
        assert!(
            err.to_string().contains("SMTP_PASSWORD"),
            "Error should mention SMTP_PASSWORD: {err}"
        );
    }

    #[test]
    fn test_new_missing_from() {
        let mut config = smtp_test_config();
        config.smtp_from = None;
        let err = expect_err(EmailService::new(&config));
        assert!(
            err.to_string().contains("SMTP_FROM"),
            "Error should mention SMTP_FROM: {err}"
        );
    }

    #[test]
    fn test_new_invalid_from_address() {
        let mut config = smtp_test_config();
        config.smtp_from = Some("not-an-email".into());
        let err = expect_err(EmailService::new(&config));
        assert!(
            err.to_string().contains("valid email"),
            "Error should mention invalid email: {err}"
        );
    }
}
