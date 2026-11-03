package io.wolftown.kaiku.ui.auth

import androidx.fragment.app.FragmentActivity
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.mockk
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.StepUpOutcome
import io.wolftown.kaiku.data.auth.DeviceSignIn
import io.wolftown.kaiku.data.auth.DeviceSignInResult
import io.wolftown.kaiku.data.auth.KeyDeviceSignOut
import io.wolftown.kaiku.data.auth.SessionEndLog
import io.wolftown.kaiku.testutil.KeyDeviceFixture
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.time.Instant

/** "Use another account" on a lapsed key phone (Argon part5 #2). */
@OptIn(ExperimentalCoroutinesApi::class)
class ConfirmItsYouViewModelTest {
    private val activity: FragmentActivity = mockk(relaxed = true)
    private val deviceSignIn: DeviceSignIn = mockk()
    private val signOut: KeyDeviceSignOut = mockk(relaxed = true)

    @Before fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())
    @After fun tearDown() = Dispatchers.resetMain()

    private fun TestScope.lapsed(): ConfirmItsYouViewModel {
        val f = KeyDeviceFixture(backgroundScope)
        f.authState.setLoggedOut() // an active key without its session: lapsed
        return ConfirmItsYouViewModel(deviceSignIn, f.repo, signOut, SessionEndLog())
    }

    @Test fun `a removal that did not happen signs the temporary session out again`() = runTest {
        val v = lapsed()
        coEvery { deviceSignIn.signIn(activity) } returns DeviceSignInResult.Success
        coEvery { signOut.signOutAndRemovePhone(activity) } returns StepUpOutcome.Cancelled
        v.removeThisPhone(activity)
        coVerify(exactly = 1) { signOut.signOut() }
        assertNull(v.ui.value.done)
    }

    @Test fun `an invalidated key during the removal says so and offers OK`() = runTest {
        val v = lapsed()
        coEvery { deviceSignIn.signIn(activity) } returns DeviceSignInResult.Success
        coEvery { signOut.signOutAndRemovePhone(activity) } returns StepUpOutcome.Failed("KEY_INVALIDATED")
        v.removeThisPhone(activity)
        assertTrue(v.ui.value.keyGone)
        assertEquals(R.string.security_key_invalidated, v.ui.value.error)
        coVerify(exactly = 1) { signOut.signOut() }
    }

    @Test fun `a pending key during the removal shows when it is ready`() = runTest {
        val v = lapsed()
        val at = Instant.parse("2026-11-05T08:00:00Z")
        coEvery { deviceSignIn.signIn(activity) } returns DeviceSignInResult.Pending(at)
        v.removeThisPhone(activity)
        assertEquals(ConfirmDone.PENDING, v.ui.value.done)
        assertEquals(at, v.ui.value.readyAt)
        coVerify(exactly = 0) { signOut.signOutAndRemovePhone(any()) }
    }

    @Test fun `a completed removal signs out for good`() = runTest {
        val v = lapsed()
        coEvery { deviceSignIn.signIn(activity) } returns DeviceSignInResult.Success
        coEvery { signOut.signOutAndRemovePhone(activity) } returns
            StepUpOutcome.Granted(io.wolftown.kaiku.domain.model.StepUpGrant("key_device.remove", "kd1", "device", "t"))
        v.removeThisPhone(activity)
        assertEquals(ConfirmDone.SIGNED_OUT, v.ui.value.done)
        coVerify(exactly = 0) { signOut.signOut() }
    }
}
