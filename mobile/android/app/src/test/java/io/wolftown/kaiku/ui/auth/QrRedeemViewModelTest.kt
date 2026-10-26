package io.wolftown.kaiku.ui.auth

import io.ktor.http.HttpStatusCode
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.mockk
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.repository.AlreadySignedInException
import io.wolftown.kaiku.data.repository.AuthRepository
import io.wolftown.kaiku.domain.model.User
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class QrRedeemViewModelTest {

    private lateinit var authRepository: AuthRepository
    private lateinit var viewModel: QrRedeemViewModel

    @Before
    fun setUp() {
        Dispatchers.setMain(StandardTestDispatcher())
        authRepository = mockk(relaxed = true)
        viewModel = QrRedeemViewModel(authRepository)
    }

    @After
    fun tearDown() {
        Dispatchers.resetMain()
    }

    @Test
    fun `successful redeem leaves no error`() = runTest {
        coEvery { authRepository.redeemQrToken("https://s", "t") } returns
            Result.success(User(id = "u", username = "anna", displayName = "Anna"))

        viewModel.start("https://s", "t")
        advanceUntilIdle()

        val state = viewModel.uiState.value
        assertTrue(state.succeeded)
        assertFalse(state.isLoading)
        assertNull(state.errorRes)
    }

    @Test
    fun `failed redeem exposes a string resource, not the server message`() = runTest {
        coEvery { authRepository.redeemQrToken("https://s", "t") } returns
            Result.failure(ApiException(HttpStatusCode.Unauthorized, "token expired", "INVALID_TOKEN"))

        viewModel.start("https://s", "t")
        advanceUntilIdle()

        val state = viewModel.uiState.value
        assertFalse(state.succeeded)
        assertFalse(state.isLoading)
        assertEquals(R.string.qr_redeem_failed, state.errorRes)
    }

    @Test
    fun `redeem while signed in says so`() = runTest {
        coEvery { authRepository.redeemQrToken("https://s", "t") } returns
            Result.failure(AlreadySignedInException())

        viewModel.start("https://s", "t")
        advanceUntilIdle()

        assertEquals(R.string.auth_login_already_signed_in, viewModel.uiState.value.errorRes)
    }

    @Test
    fun `starting again after a configuration change does not redeem twice`() = runTest {
        coEvery { authRepository.redeemQrToken("https://s", "t") } returns
            Result.failure(ApiException(HttpStatusCode.Unauthorized, "used", "INVALID_TOKEN"))

        viewModel.start("https://s", "t")
        advanceUntilIdle()
        viewModel.start("https://s", "t")
        advanceUntilIdle()

        coVerify(exactly = 1) { authRepository.redeemQrToken(any(), any()) }
        assertEquals(R.string.qr_redeem_failed, viewModel.uiState.value.errorRes)
    }
}
