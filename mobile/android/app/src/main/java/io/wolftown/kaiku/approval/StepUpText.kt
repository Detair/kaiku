package io.wolftown.kaiku.approval

import io.wolftown.kaiku.R
import io.wolftown.kaiku.domain.model.StepUpAction

/**
 * The UV prompt's subtitle for a step-up: what is being confirmed, with the
 * server's `target_label` from the verified context where the action has one.
 */
object StepUpText {
    /** The string and whether it takes the target label as `%1$s`. */
    fun prompt(action: StepUpAction): Pair<Int, Boolean> = when (action) {
        StepUpAction.PASSKEY_ADD -> R.string.security_step_up_passkey_add to false
        StepUpAction.PASSKEY_REMOVE -> R.string.security_step_up_passkey_remove to true
        StepUpAction.KEY_DEVICE_REMOVE -> R.string.security_step_up_key_device_remove to true
        StepUpAction.ENROLLMENT_ISSUE -> R.string.security_step_up_enrollment_issue to false
        StepUpAction.PASSWORD_SET -> R.string.security_step_up_password_set to false
        StepUpAction.PASSWORD_REMOVE -> R.string.security_step_up_password_remove to false
        StepUpAction.TOTP_SET -> R.string.security_step_up_totp_set to false
        StepUpAction.TOTP_REMOVE -> R.string.security_step_up_totp_remove to false
        StepUpAction.RECOVERY_CODES_CREATE -> R.string.security_step_up_recovery_codes_create to false
        StepUpAction.IDENTITY_LINK -> R.string.security_step_up_identity_link to false
        StepUpAction.IDENTITY_UNLINK -> R.string.security_step_up_identity_unlink to true
        StepUpAction.SESSIONS_END -> R.string.security_step_up_sessions_end to true
        StepUpAction.RECOVERY_CONTACT_SET -> R.string.security_step_up_recovery_contact_set to true
        StepUpAction.REQUEST_CANCEL -> R.string.security_step_up_request_cancel to true
        StepUpAction.EVENT_REPUDIATE -> R.string.security_step_up_event_repudiate to true
        StepUpAction.ACCOUNT_DELETE -> R.string.security_step_up_account_delete to false
        StepUpAction.ADMIN_ELEVATE -> R.string.security_step_up_admin_elevate to false
        StepUpAction.ADMIN_ENROLLMENT_ISSUE -> R.string.security_step_up_admin_enrollment_issue to true
        StepUpAction.ADMIN_AUTHENTICATOR_REMOVE -> R.string.security_step_up_admin_authenticator_remove to true
        StepUpAction.ADMIN_MAY_APPROVE -> R.string.security_step_up_admin_may_approve to true
        StepUpAction.ADMIN_FACTOR_SET_STATE -> R.string.security_step_up_admin_factor_set_state to true
        StepUpAction.ADMIN_RECOVERY_CONTACT_SET -> R.string.security_step_up_admin_recovery_contact_set to true
    }
}
