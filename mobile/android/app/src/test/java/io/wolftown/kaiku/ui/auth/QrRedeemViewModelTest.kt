package io.wolftown.kaiku.ui.auth

import io.ktor.http.HttpStatusCode
import io.mockk.coEvery
import io.mockk.mockk
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.repository.AuthRepository
import io.wolftown.kaiku.domain.model.User
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class QrRedeemViewModelTest {

    private val authRepository: AuthRepository = mockk(relaxed = true)
    private val viewModel = QrRedeemViewModel(authRepository)

    @Test
    fun `successful redeem leaves no error`() = runTest {
        coEvery { authRepository.redeemQrToken("https://s", "t") } returns
            Result.success(User(id = "u", username = "anna", displayName = "Anna"))

        assertTrue(viewModel.redeem("https://s", "t"))
        assertNull(viewModel.errorRes.value)
    }

    @Test
    fun `failed redeem exposes a string resource, not the server message`() = runTest {
        coEvery { authRepository.redeemQrToken("https://s", "t") } returns
            Result.failure(ApiException(HttpStatusCode.Unauthorized, "token expired", "INVALID_TOKEN"))

        assertTrue(!viewModel.redeem("https://s", "t"))
        assertEquals(R.string.qr_redeem_failed, viewModel.errorRes.value)
    }

    @Test
    fun `redeem while signed in says so`() = runTest {
        coEvery { authRepository.redeemQrToken("https://s", "t") } returns
            Result.failure(io.wolftown.kaiku.data.repository.AlreadySignedInException())

        viewModel.redeem("https://s", "t")
        assertEquals(R.string.auth_login_already_signed_in, viewModel.errorRes.value)
    }
}
