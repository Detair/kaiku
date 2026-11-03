package io.wolftown.kaiku.ui.security

import androidx.annotation.StringRes
import io.wolftown.kaiku.R

/**
 * C28 text keys (C15): web `security.notice.<eventType>` / `security.push.<eventType>`
 * map to these Android names. Literal `when`s, so every string is referenced; an
 * unknown event type uses the generic text. Arguments, in this order:
 * `%1$s` subject, `%2$s` actor, `%3$s` device, `%4$d` count.
 */
object NoticeText {
    @StringRes
    fun title(eventType: String): Int = when (eventType) {
        "sign_in" -> R.string.security_notice_sign_in
        "sign_in_not_finished" -> R.string.security_notice_sign_in_not_finished
        "second_factor_failed" -> R.string.security_notice_second_factor_failed
        "guessing" -> R.string.security_notice_guessing
        "approval_reported" -> R.string.security_notice_approval_reported
        "authenticator_added" -> R.string.security_notice_authenticator_added
        "authenticator_removed" -> R.string.security_notice_authenticator_removed
        "key_invalidated" -> R.string.security_notice_key_invalidated
        "recovery_codes_created" -> R.string.security_notice_recovery_codes_created
        "password_set" -> R.string.security_notice_password_set
        "password_changed" -> R.string.security_notice_password_changed
        "password_removed" -> R.string.security_notice_password_removed
        "password_throttled" -> R.string.security_notice_password_throttled
        "password_compromised" -> R.string.security_notice_password_compromised
        "totp_throttled" -> R.string.security_notice_totp_throttled
        "totp_compromised" -> R.string.security_notice_totp_compromised
        "step_up_action" -> R.string.security_notice_step_up_action
        "repudiated" -> R.string.security_notice_repudiated
        "request_started" -> R.string.security_notice_request_started
        "request_completed" -> R.string.security_notice_request_completed
        "request_cancelled" -> R.string.security_notice_request_cancelled
        "request_withdrawn" -> R.string.security_notice_request_withdrawn
        "request_ready_soon" -> R.string.security_notice_request_ready_soon
        "request_ready" -> R.string.security_notice_request_ready
        "request_expired" -> R.string.security_notice_request_expired
        "request_void" -> R.string.security_notice_request_void
        "admin_setup" -> R.string.security_notice_admin_setup
        "admin_role_granted" -> R.string.security_notice_admin_role_granted
        "admin_role_removed" -> R.string.security_notice_admin_role_removed
        "recovery_contact_named" -> R.string.security_notice_recovery_contact_named
        "recovery_contact_confirmed" -> R.string.security_notice_recovery_contact_confirmed
        "recovery_contact_changed" -> R.string.security_notice_recovery_contact_changed
        "session_ended_reuse" -> R.string.security_notice_session_ended_reuse
        "session_ended_limit" -> R.string.security_notice_session_ended_limit
        "bootstrap_reminder" -> R.string.security_notice_bootstrap_reminder
        else -> R.string.security_notice_generic
    }

    @StringRes
    fun push(eventType: String): Int = when (eventType) {
        "sign_in" -> R.string.security_push_sign_in
        "sign_in_not_finished" -> R.string.security_push_sign_in_not_finished
        "second_factor_failed" -> R.string.security_push_second_factor_failed
        "guessing" -> R.string.security_push_guessing
        "approval_reported" -> R.string.security_push_approval_reported
        "authenticator_added" -> R.string.security_push_authenticator_added
        "authenticator_removed" -> R.string.security_push_authenticator_removed
        "key_invalidated" -> R.string.security_push_key_invalidated
        "recovery_codes_created" -> R.string.security_push_recovery_codes_created
        "password_set" -> R.string.security_push_password_set
        "password_changed" -> R.string.security_push_password_changed
        "password_removed" -> R.string.security_push_password_removed
        "password_throttled" -> R.string.security_push_password_throttled
        "password_compromised" -> R.string.security_push_password_compromised
        "totp_throttled" -> R.string.security_push_totp_throttled
        "totp_compromised" -> R.string.security_push_totp_compromised
        "step_up_action" -> R.string.security_push_step_up_action
        "repudiated" -> R.string.security_push_repudiated
        "request_started" -> R.string.security_push_request_started
        "request_completed" -> R.string.security_push_request_completed
        "request_cancelled" -> R.string.security_push_request_cancelled
        "request_withdrawn" -> R.string.security_push_request_withdrawn
        "request_ready_soon" -> R.string.security_push_request_ready_soon
        "request_ready" -> R.string.security_push_request_ready
        "request_expired" -> R.string.security_push_request_expired
        "request_void" -> R.string.security_push_request_void
        "admin_setup" -> R.string.security_push_admin_setup
        "admin_role_granted" -> R.string.security_push_admin_role_granted
        "admin_role_removed" -> R.string.security_push_admin_role_removed
        "recovery_contact_named" -> R.string.security_push_recovery_contact_named
        "recovery_contact_confirmed" -> R.string.security_push_recovery_contact_confirmed
        "recovery_contact_changed" -> R.string.security_push_recovery_contact_changed
        "session_ended_reuse" -> R.string.security_push_session_ended_reuse
        "session_ended_limit" -> R.string.security_push_session_ended_limit
        "bootstrap_reminder" -> R.string.security_push_bootstrap_reminder
        else -> R.string.security_push_generic
    }
}
