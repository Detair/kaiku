package io.wolftown.kaiku.data.auth

import androidx.fragment.app.FragmentActivity
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.mockk
import io.wolftown.kaiku.approval.DeviceStepUp
import io.wolftown.kaiku.approval.SoftwareApprovalKeyStore
import io.wolftown.kaiku.approval.StepUpOutcome
import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.approval.UvDecision
import io.wolftown.kaiku.approval.UvPolicy
import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.KeyDeviceState
import io.wolftown.kaiku.data.keydevice.LocalKeyDevice
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.AuthRepository
import io.wolftown.kaiku.domain.model.StepUpAction
import io.wolftown.kaiku.domain.model.StepUpGrant
import io.wolftown.kaiku.push.PushRegistrar
import io.wolftown.kaiku.testutil.FakeApprovalApi
import io.wolftown.kaiku.testutil.InMemoryKeyDeviceStore
import io.wolftown.kaiku.testutil.noTeardown
import io.wolftown.kaiku.testutil.InMemorySharedPreferences
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class KeyDeviceSignOutTest {

    private val activity: FragmentActivity = mockk(relaxed = true)

    @Test fun `sign out keeps the key and sign out and remove deletes it`() = runTest {
        val keyStore = SoftwareApprovalKeyStore().apply {
            generate(ByteArray(32), UvPolicy.choose(30, true, true) as UvDecision.Use)
        }
        val authState = AuthState(backgroundScope)
        val tokenStorage = TokenStorage(InMemorySharedPreferences())
        tokenStorage.saveServerUrl("https://chat.example.com")
        val repo = KeyDeviceRepository(InMemoryKeyDeviceStore(), keyStore, authState, backgroundScope)
        val pushRegistrar: PushRegistrar = mockk(relaxed = true)
        val authApi: AuthApi = mockk(relaxed = true)
        val authRepository = AuthRepository(authApi, tokenStorage, authState, pushRegistrar, noTeardown(authState))
        authRepository.keyDevicePolicy = repo
        val stepUp: DeviceStepUp = mockk()
        val api = FakeApprovalApi()
        val signOut = KeyDeviceSignOut(authRepository, repo, stepUp, api)

        fun signIn() {
            tokenStorage.saveTokens("a", "r", 900, "u1")
            authState.setLoggedIn("u1")
        }
        repo.saveClaimed(LocalKeyDevice("kd1", "u1", "https://chat.example.com", UvClass.PER_USE, "e1", null))
        signIn()
        repo.markActive("u1", "kd1")

        // "Sign out": the session ends, the key and the push subscription stay.
        signOut.signOut()
        assertEquals(KeyDeviceState.Lapsed("u1"), repo.current())
        assertTrue(keyStore.hasKey())
        coVerify(exactly = 0) { pushRegistrar.unregisterForLogout(any()) }

        // "Sign out and remove this phone": step-up for the own key device, DELETE, forget, push off.
        signIn()
        coEvery { stepUp.stepUp(activity, StepUpAction.KEY_DEVICE_REMOVE, "kd1", any()) } returns
            StepUpOutcome.Granted(StepUpGrant("key_device.remove", "kd1", "device", "t"))
        assertEquals(StepUpOutcome.Granted(StepUpGrant("key_device.remove", "kd1", "device", "t")), signOut.signOutAndRemovePhone(activity))
        assertTrue("removeKeyDevice:kd1" in api.calls)
        assertEquals(KeyDeviceState.None, repo.current())
        assertFalse(keyStore.hasKey())
        coVerify(exactly = 1) { pushRegistrar.unregisterForLogout(any()) }
        assertEquals(io.wolftown.kaiku.data.local.AuthSession.LoggedOut, authState.session.value)
    }

    @Test fun `remove this phone without a grant changes nothing`() = runTest {
        val keyStore = SoftwareApprovalKeyStore().apply {
            generate(ByteArray(32), UvPolicy.choose(30, true, true) as UvDecision.Use)
        }
        val authState = AuthState(backgroundScope)
        val repo = KeyDeviceRepository(InMemoryKeyDeviceStore(), keyStore, authState, backgroundScope)
        repo.saveClaimed(LocalKeyDevice("kd1", "u1", "https://chat.example.com", UvClass.PER_USE, "e1", null))
        authState.setLoggedIn("u1")
        repo.markActive("u1", "kd1")
        val stepUp: DeviceStepUp = mockk()
        coEvery { stepUp.stepUp(any(), any(), any(), any()) } returns StepUpOutcome.Cancelled
        val api = FakeApprovalApi()
        val signOut = KeyDeviceSignOut(mockk(relaxed = true), repo, stepUp, api)
        assertEquals(StepUpOutcome.Cancelled, signOut.signOutAndRemovePhone(activity))
        assertTrue(api.calls.isEmpty())
        assertEquals(KeyDeviceState.Active("u1"), repo.current())
    }
}
