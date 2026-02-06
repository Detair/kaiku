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
}
