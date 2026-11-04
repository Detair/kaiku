package io.wolftown.kaiku.ui.shared

import io.mockk.coEvery
import io.mockk.mockk
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
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.io.IOException

@OptIn(ExperimentalCoroutinesApi::class)
class ProbationBannerTest {

    private val authApi: AuthApi = mockk()

    @Before fun setUp() = Dispatchers.setMain(StandardTestDispatcher())
    @After fun tearDown() = Dispatchers.resetMain()

    private fun me(session: SessionInfo?) = User("u-1", "anna", "Anna", session = session)

    @Test fun `probation session shows the banner`() = runTest {
        coEvery { authApi.getMe() } returns me(SessionInfo("s-1", assurance = "probation", authMethods = listOf("password", "phone_confirm")))
        val vm = ProbationBannerViewModel(authApi)
        advanceUntilIdle()
        assertTrue(vm.visible.value)
    }

    @Test fun `a full session shows no banner`() = runTest {
        coEvery { authApi.getMe() } returns me(SessionInfo("s-1", assurance = "full", authMethods = listOf("device_key")))
        val vm = ProbationBannerViewModel(authApi)
        advanceUntilIdle()
        assertFalse(vm.visible.value)
    }

    @Test fun `a profile without a session counts as probation`() = runTest {
        coEvery { authApi.getMe() } returns me(null)
        val vm = ProbationBannerViewModel(authApi)
        advanceUntilIdle()
        assertTrue(vm.visible.value)
        assertTrue(isProbation(me(SessionInfo("s-1", assurance = "unknown-class"))))
    }

    @Test fun `an unreadable profile shows no banner`() = runTest {
        coEvery { authApi.getMe() } throws IOException("offline")
        val vm = ProbationBannerViewModel(authApi)
        advanceUntilIdle()
        assertFalse(vm.visible.value)
    }

    @Test fun `a passing network error does not hide the banner for good`() = runTest {
        coEvery { authApi.getMe() } throws IOException("offline")
        val vm = ProbationBannerViewModel(authApi)
        advanceUntilIdle()
        assertFalse(vm.visible.value)
        coEvery { authApi.getMe() } returns me(SessionInfo("s-1", assurance = "probation"))
        vm.refresh() // home resumed
        advanceUntilIdle()
        assertTrue(vm.visible.value)
    }
}
