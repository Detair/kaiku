//! `kaiku-admin enroll <username>`: a key phone for any account, active at
//! once (SM2 variant `cli`; F6 item 4).
//!
//! The host prints a setup link (and its QR code), waits for the phone to
//! claim it, shows the label the phone sent, and asks for the code the phone
//! shows. Three wrong codes fail the enrolment.

use std::time::Duration;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::{account, audit, CliCtx, CliIo};
use crate::approvals::context::ClaimantCheck;
use crate::approvals::enrollments::{
    after_done, confirm_tx, issue_tx, Confirmer, EnrollmentVariant, IssueParams, Issuer,
};
use crate::approvals::error::ApprovalError;

/// How often [`wait_for_claim`] looks at the enrolment.
const POLL: Duration = Duration::from_millis(500);

/// An issued CLI enrolment. Its `Debug` redacts the setup link.
#[derive(Clone)]
pub struct CliEnrollment {
    pub enrollment_id: Uuid,
    /// The setup link (carries the setup token; printed once).
    pub setup_url: String,
    pub expires_at: DateTime<Utc>,
}

impl std::fmt::Debug for CliEnrollment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CliEnrollment")
            .field("enrollment_id", &self.enrollment_id)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

/// What the claiming phone said about itself.
#[derive(Debug, Clone)]
pub struct ClaimedInfo {
    /// The phone's self-reported label, cleaned at intake (no control or
    /// bidi characters, so it is safe to print).
    pub label: String,
}

/// Issue a `cli` enrolment for `username` (any account, admins included).
pub async fn issue(ctx: &CliCtx, username: &str) -> anyhow::Result<CliEnrollment> {
    let auth = ctx.auth();
    let mut tx = ctx.db.begin().await?;
    let user_id = account(&mut tx, username).await?;
    let issued = issue_tx(
        &mut tx,
        &auth,
        IssueParams {
            variant: EnrollmentVariant::Cli,
            subject_id: user_id,
            issuer: Issuer::Cli,
            claimant: ClaimantCheck::None,
            claimant_note: None,
            lost_device_ids: Vec::new(),
        },
    )
    .await?;
    audit(
        &mut tx,
        "cli.enroll",
        user_id,
        serde_json::json!({ "enrollment_id": issued.enrollment_id }),
    )
    .await?;
    tx.commit().await?;
    Ok(CliEnrollment {
        enrollment_id: issued.enrollment_id,
        setup_url: issued.setup_url,
        expires_at: issued.expires_at,
    })
}

/// Wait until a phone claimed enrolment `id` (or it ended, or `timeout`).
pub async fn wait_for_claim(
    ctx: &CliCtx,
    id: Uuid,
    timeout: Duration,
) -> anyhow::Result<ClaimedInfo> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let (state, label, expires_at): (String, Option<String>, DateTime<Utc>) = sqlx::query_as(
            "SELECT state, claimed_label, expires_at FROM enrollments WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&ctx.db)
        .await?;
        match state.as_str() {
            "claimed" => {
                return Ok(ClaimedInfo {
                    label: label.unwrap_or_default(),
                })
            }
            "issued" if expires_at > Utc::now() => {}
            "issued" => anyhow::bail!("The setup link expired before a phone opened it."),
            other => anyhow::bail!("The setup ended ({other})."),
        }
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!("No phone opened the setup link in time.");
        }
        tokio::time::sleep(POLL).await;
    }
}

/// Confirm enrolment `id` with the code the phone shows (spaces ignored).
///
/// A wrong code spends a try (`ApprovalError::EnrollCodeWrong`, then
/// `EnrollmentFailed`); the error is returned as an [`anyhow::Error`]
/// wrapping the [`ApprovalError`].
pub async fn confirm(ctx: &CliCtx, id: Uuid, code: &str) -> anyhow::Result<()> {
    let digits: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    let auth = ctx.auth();
    let mut tx = ctx.db.begin().await?;
    match confirm_tx(&mut tx, &auth, id, Confirmer::Cli, digits.as_bytes()).await {
        Ok((_, after)) => {
            // Built before the commit: a failure here must not skip the
            // post-commit steps of a committed enrolment.
            let state = ctx.app_state()?;
            tx.commit().await?;
            after_done(&state, after).await;
            Ok(())
        }
        // A spent try, a failed or an expired enrolment are kept.
        Err(
            e @ (ApprovalError::EnrollCodeWrong { .. }
            | ApprovalError::EnrollmentFailed
            | ApprovalError::EnrollmentState),
        ) => {
            tx.commit().await?;
            Err(e.into())
        }
        Err(e) => Err(e.into()),
    }
}

fn tries_left(n: i16) -> String {
    if n == 1 {
        "1 try left".to_string()
    } else {
        format!("{n} tries left")
    }
}

/// The QR code of `text` for a terminal: two module rows per line, light
/// modules (and the quiet zone) drawn, so it scans on a dark background.
#[must_use]
pub fn terminal_qr(text: &str) -> String {
    use qrcodegen::{QrCode, QrCodeEcc};
    let Ok(qr) = QrCode::encode_text(text, QrCodeEcc::Low) else {
        return String::new();
    };
    let quiet = 2;
    let size = qr.size();
    let light = |x: i32, y: i32| !qr.get_module(x, y);
    let mut out = String::new();
    let mut y = -quiet;
    while y < size + quiet {
        for x in -quiet..size + quiet {
            out.push(match (light(x, y), light(x, y + 1)) {
                (true, true) => '█',
                (true, false) => '▀',
                (false, true) => '▄',
                (false, false) => ' ',
            });
        }
        out.push('\n');
        y += 2;
    }
    out
}

/// The whole `enroll` dialogue.
pub async fn run(ctx: &CliCtx, username: &str, io: &mut impl CliIo) -> anyhow::Result<()> {
    let e = issue(ctx, username).await?;
    io.print(&format!(
        "Open this link on the phone that should become {username}'s key (valid until {}):",
        e.expires_at.format("%H:%M UTC")
    ));
    io.print(&e.setup_url);
    io.print(&terminal_qr(&e.setup_url));
    io.print("Waiting for the phone…");
    let wait = (e.expires_at - Utc::now())
        .to_std()
        .unwrap_or(Duration::ZERO);
    let claimed = wait_for_claim(ctx, e.enrollment_id, wait).await?;
    io.print(&format!(
        "A device calling itself '{}' wants to become {username}'s key.",
        claimed.label
    ));
    io.print("Only continue if it is the phone in front of you. Type the code it shows.");
    loop {
        let line = io.read_line("Code: ").await?;
        let digits: String = line.chars().filter(|c| !c.is_whitespace()).collect();
        if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_digit()) {
            io.print("Type the 6 digits the phone shows.");
            continue;
        }
        match confirm(ctx, e.enrollment_id, &digits).await {
            Ok(()) => {
                io.print(&format!("Done: the phone is now {username}'s key."));
                return Ok(());
            }
            Err(err) => match err.downcast_ref::<ApprovalError>() {
                Some(ApprovalError::EnrollCodeWrong { tries_left: n }) => {
                    io.print(&format!("Wrong code, {}.", tries_left(*n)));
                }
                Some(ApprovalError::EnrollmentFailed) => {
                    anyhow::bail!("Too many wrong codes: the setup failed. Start again.")
                }
                Some(ApprovalError::EnrollmentState) => {
                    anyhow::bail!("The code expired: the setup ended. Start again.")
                }
                _ => return Err(err),
            },
        }
    }
}
