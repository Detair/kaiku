package io.wolftown.kaiku.ui.auth

import androidx.annotation.StringRes
import io.ktor.http.HttpStatusCode
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.repository.AlreadySignedInException
import java.io.IOException

/**
 * Maps sign-in failures to user-facing copy by exception type, status and error code (C16).
 * The exception message (which may be server text) is never shown.
 */
object AuthErrorMapper {

    @StringRes
    fun login(e: Throwable): Int {
        if (e is ApiException) {
            when {
                e.normalizedCode == "INVALID_MFA" -> return R.string.auth_login_mfa_invalid
                e.normalizedCode == "INVALID_CREDENTIALS" || e.status == HttpStatusCode.Unauthorized ->
                    return R.string.auth_login_invalid_credentials
            }
        }
        return common(e) ?: R.string.auth_login_failed
    }

    @StringRes
    fun register(e: Throwable): Int {
        if (e is ApiException) {
            when (e.normalizedCode) {
                "USER_EXISTS" -> return R.string.auth_register_username_taken
                "EMAIL_TAKEN" -> return R.string.auth_register_email_taken
                "REGISTRATION_DISABLED" -> return R.string.auth_register_closed
            }
        }
        return common(e) ?: R.string.auth_register_failed
    }

    @StringRes
    fun oidc(e: Throwable): Int = common(e) ?: R.string.auth_login_sso_failed

    /** QR sign-in: a rejected or expired code has its own copy. */
    @StringRes
    fun qrRedeem(e: Throwable): Int = common(e) ?: R.string.qr_redeem_failed

    /** Failures that read the same in every flow, or null when the flow's own fallback applies. */
    @StringRes
    private fun common(e: Throwable): Int? = when {
        e is AlreadySignedInException -> R.string.auth_login_already_signed_in
        e is ApiException && e.status == HttpStatusCode.TooManyRequests -> R.string.errors_rate_limited
        e is ApiException && e.status.value >= 500 -> R.string.errors_server
        e is IOException -> R.string.errors_network
        else -> null
    }
}
