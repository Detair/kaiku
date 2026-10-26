package io.wolftown.kaiku.ui.auth

import io.ktor.http.HttpStatusCode
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.repository.AlreadySignedInException
import org.junit.Assert.assertEquals
import org.junit.Test
import java.io.IOException

class AuthErrorMapperTest {

    @Test
    fun `invalid credentials maps by code`() {
        val e = ApiException(HttpStatusCode.Unauthorized, "server text", "INVALID_CREDENTIALS")
        assertEquals(R.string.auth_login_invalid_credentials, AuthErrorMapper.login(e))
    }

    @Test
    fun `401 without code maps to invalid credentials`() {
        val e = ApiException(HttpStatusCode.Unauthorized, "server text")
        assertEquals(R.string.auth_login_invalid_credentials, AuthErrorMapper.login(e))
    }

    @Test
    fun `invalid mfa code maps to mfa invalid`() {
        val e = ApiException(HttpStatusCode.Unauthorized, "server text", "INVALID_MFA")
        assertEquals(R.string.auth_login_mfa_invalid, AuthErrorMapper.login(e))
    }

    @Test
    fun `registration disabled maps to closed`() {
        val e = ApiException(HttpStatusCode.Forbidden, "server text", "REGISTRATION_DISABLED")
        assertEquals(R.string.auth_register_closed, AuthErrorMapper.register(e))
    }

    @Test
    fun `existing username and email map to their own copy`() {
        val user = ApiException(HttpStatusCode.Conflict, "server text", "USER_EXISTS")
        val email = ApiException(HttpStatusCode.Conflict, "server text", "EMAIL_TAKEN")
        assertEquals(R.string.auth_register_username_taken, AuthErrorMapper.register(user))
        assertEquals(R.string.auth_register_email_taken, AuthErrorMapper.register(email))
    }

    @Test
    fun `io exception maps to network`() {
        val e = IOException("timeout")
        assertEquals(R.string.errors_network, AuthErrorMapper.login(e))
        assertEquals(R.string.errors_network, AuthErrorMapper.register(e))
        assertEquals(R.string.errors_network, AuthErrorMapper.oidc(e))
    }

    @Test
    fun `429 and 5xx map to rate limited and server`() {
        val limited = ApiException(HttpStatusCode.TooManyRequests, "server text")
        val broken = ApiException(HttpStatusCode.BadGateway, "server text")
        assertEquals(R.string.errors_rate_limited, AuthErrorMapper.login(limited))
        assertEquals(R.string.errors_rate_limited, AuthErrorMapper.register(limited))
        assertEquals(R.string.errors_server, AuthErrorMapper.login(broken))
        assertEquals(R.string.errors_server, AuthErrorMapper.oidc(broken))
    }

    @Test
    fun `unknown 418 maps to login failed`() {
        val e = ApiException(HttpStatusCode(418, "I'm a teapot"), "server text", "SOMETHING")
        assertEquals(R.string.auth_login_failed, AuthErrorMapper.login(e))
        assertEquals(R.string.auth_register_failed, AuthErrorMapper.register(e))
        assertEquals(R.string.auth_login_sso_failed, AuthErrorMapper.oidc(e))
    }

    @Test
    fun `unknown exception maps to the flow's failure copy`() {
        val e = IllegalArgumentException("boom")
        assertEquals(R.string.auth_login_failed, AuthErrorMapper.login(e))
        assertEquals(R.string.auth_register_failed, AuthErrorMapper.register(e))
        assertEquals(R.string.auth_login_sso_failed, AuthErrorMapper.oidc(e))
    }

    @Test
    fun `already signed in maps in every flow`() {
        val e = AlreadySignedInException()
        assertEquals(R.string.auth_login_already_signed_in, AuthErrorMapper.login(e))
        assertEquals(R.string.auth_login_already_signed_in, AuthErrorMapper.register(e))
        assertEquals(R.string.auth_login_already_signed_in, AuthErrorMapper.oidc(e))
        assertEquals(R.string.auth_login_already_signed_in, AuthErrorMapper.qrRedeem(e))
    }

    @Test
    fun `qr redeem maps network and falls back to the qr copy`() {
        assertEquals(R.string.errors_network, AuthErrorMapper.qrRedeem(IOException("down")))
        val e = ApiException(HttpStatusCode.Unauthorized, "server text", "INVALID_TOKEN")
        assertEquals(R.string.qr_redeem_failed, AuthErrorMapper.qrRedeem(e))
    }
}
