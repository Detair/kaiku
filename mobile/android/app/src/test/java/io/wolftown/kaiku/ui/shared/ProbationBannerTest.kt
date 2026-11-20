package io.wolftown.kaiku.ui.shared

import io.mockk.coEvery
import io.mockk.mockk
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.domain.model.SessionInfo
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
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.io.IOException

@OptIn(ExperimentalCoroutinesApi::class)
class ProbationBannerTest {

    private val authApi: AuthApi = mockk()
    private val securityApi: io.wolftown.kaiku.data.api.SecurityApi = mockk()

    @Before fun setUp() {
        Dispatchers.setMain(StandardTestDispatcher())
        coEvery { securityApi.meSecurity() } returns io.wolftown.kaiku.domain.model.MeSecurity()
    }
    @After fun tearDown() = Dispatchers.resetMain()

    private fun me(session: SessionInfo?, mayApprove: Boolean = false) =
        User("u-1", "anna", "Anna", session = session, mayApprove = mayApprove)

    private fun security(keyDevices: Int, mayApprove: Boolean, passkeys: Int) =
        io.wolftown.kaiku.domain.model.MeSecurity(keyDevices = keyDevices, mayApprove = mayApprove, passkeys = passkeys)

    @Test fun `the text names only ways the account has`() = runTest {
        coEvery { authApi.getMe() } returns me(SessionInfo("s-1", assurance = "probation"), mayApprove = true)
        val cases = listOf(
            security(1, true, 1) to R.string.auth_probation_banner,
            security(1, true, 0) to R.string.auth_probation_banner_add_phone,
            // may_approve is the account's flag: without a key phone there is no "Add a phone".
            security(0, true, 1) to R.string.auth_probation_banner_passkey,
            security(1, false, 1) to R.string.auth_probation_banner_passkey,
            security(0, true, 0) to R.string.auth_probation_banner_ask_admin,
        )
        cases.forEach { (sec, expected) ->
            coEvery { securityApi.meSecurity() } returns sec
            val vm = ProbationBannerViewModel(authApi, securityApi)
            advanceUntilIdle()
            assertTrue(vm.visible.value)
            assertEquals(sec.toString(), expected, vm.text.value)
        }
    }

    @Test fun `without the overview the text names no way`() = runTest {
        coEvery { authApi.getMe() } returns me(SessionInfo("s-1", assurance = "probation"))
        coEvery { securityApi.meSecurity() } throws IOException("offline")
        val vm = ProbationBannerViewModel(authApi, securityApi)
        advanceUntilIdle()
        assertTrue(vm.visible.value)
        assertEquals(R.string.auth_probation_banner_neutral, vm.text.value)
    }

    @Test fun `probation session shows the banner`() = runTest {
        coEvery { authApi.getMe() } returns me(SessionInfo("s-1", assurance = "probation", authMethods = listOf("password", "phone_confirm")))
        val vm = ProbationBannerViewModel(authApi, securityApi)
        advanceUntilIdle()
        assertTrue(vm.visible.value)
    }

    @Test fun `a full session shows no banner`() = runTest {
        coEvery { authApi.getMe() } returns me(SessionInfo("s-1", assurance = "full", authMethods = listOf("device_key")))
        val vm = ProbationBannerViewModel(authApi, securityApi)
        advanceUntilIdle()
        assertFalse(vm.visible.value)
    }

    @Test fun `a profile without a session counts as probation`() = runTest {
        coEvery { authApi.getMe() } returns me(null)
        val vm = ProbationBannerViewModel(authApi, securityApi)
        advanceUntilIdle()
        assertTrue(vm.visible.value)
        assertTrue(isProbation(me(SessionInfo("s-1", assurance = "unknown-class"))))
    }

    @Test fun `an unreadable profile shows no banner`() = runTest {
        coEvery { authApi.getMe() } throws IOException("offline")
        val vm = ProbationBannerViewModel(authApi, securityApi)
        advanceUntilIdle()
        assertFalse(vm.visible.value)
    }

    @Test fun `a passing network error does not hide the banner for good`() = runTest {
        coEvery { authApi.getMe() } throws IOException("offline")
        val vm = ProbationBannerViewModel(authApi, securityApi)
        advanceUntilIdle()
        assertFalse(vm.visible.value)
        coEvery { authApi.getMe() } returns me(SessionInfo("s-1", assurance = "probation"))
        vm.refresh() // home resumed
        advanceUntilIdle()
        assertTrue(vm.visible.value)
    }
}
