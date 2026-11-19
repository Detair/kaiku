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
import kotlinx.coroutines.cancel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runCurrent
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
    private val probe: io.wolftown.kaiku.data.keydevice.KeyDeviceProbe = mockk {
        coEvery { probe(any(), any()) } returns io.wolftown.kaiku.data.keydevice.ServerKeyState.Active
    }

    @Before fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())
    @After fun tearDown() = Dispatchers.resetMain()

    private fun TestScope.lapsed(): ConfirmItsYouViewModel {
        val f = KeyDeviceFixture(backgroundScope)
        f.authState.setLoggedOut() // an active key without its session: lapsed
        return ConfirmItsYouViewModel(deviceSignIn, f.repo, signOut, SessionEndLog(), probe)
    }

    @Test fun `a removal that did not happen signs the temporary session out again`() = runTest {
        val v = lapsed()
        coEvery { deviceSignIn.signIn(activity, any()) } returns DeviceSignInResult.Success
        coEvery { signOut.signOutAndRemovePhone(activity) } returns StepUpOutcome.Cancelled
        v.removeThisPhone(activity)
        coVerify(exactly = 1) { signOut.signOut() }
        assertNull(v.ui.value.done)
    }

    @Test fun `an invalidated key during the removal says so and offers OK`() = runTest {
        val v = lapsed()
        coEvery { deviceSignIn.signIn(activity, any()) } returns DeviceSignInResult.Success
        coEvery { signOut.signOutAndRemovePhone(activity) } returns StepUpOutcome.Failed("KEY_INVALIDATED")
        v.removeThisPhone(activity)
        assertTrue(v.ui.value.keyGone)
        assertEquals(R.string.security_key_invalidated, v.ui.value.error)
        coVerify(exactly = 1) { signOut.signOut() }
    }

    @Test fun `a pending key during the removal shows when it is ready`() = runTest {
        val v = lapsed()
        val at = Instant.parse("2026-11-05T08:00:00Z")
        coEvery { deviceSignIn.signIn(activity, any()) } returns DeviceSignInResult.Pending(at)
        v.removeThisPhone(activity)
        assertEquals(ConfirmDone.PENDING, v.ui.value.done)
        assertEquals(at, v.ui.value.readyAt)
        coVerify(exactly = 0) { signOut.signOutAndRemovePhone(any()) }
    }

    @Test fun `a completed removal signs out for good`() = runTest {
        val v = lapsed()
        coEvery { deviceSignIn.signIn(activity, any()) } returns DeviceSignInResult.Success
        coEvery { signOut.signOutAndRemovePhone(activity) } returns
            StepUpOutcome.Granted(io.wolftown.kaiku.domain.model.StepUpGrant("key_device.remove", "kd1", "device", "t"))
        v.removeThisPhone(activity)
        assertEquals(ConfirmDone.SIGNED_OUT, v.ui.value.done)
        coVerify(exactly = 0) { signOut.signOut() }
    }

    @Test fun `the only key of the account is kept and the reason is shown`() = runTest {
        // The server refuses to remove the last strong authenticator (409 LAST_STRONG_AUTHENTICATOR).
        val f = KeyDeviceFixture(backgroundScope)
        f.authState.setLoggedOut()
        val v = ConfirmItsYouViewModel(deviceSignIn, f.repo, signOut, SessionEndLog(), probe)
        coEvery { deviceSignIn.signIn(activity, any()) } returns DeviceSignInResult.Success
        coEvery { signOut.signOutAndRemovePhone(activity) } returns StepUpOutcome.Failed("LAST_STRONG_AUTHENTICATOR")
        v.removeThisPhone(activity)
        // The session made only for the removal ends; the phone stays the key ("Sign out" state).
        coVerify(exactly = 1) { signOut.signOut() }
        assertEquals(R.string.security_only_key, v.ui.value.error)
        assertEquals(false, v.ui.value.keyGone)
        assertNull(v.ui.value.done)
        assertEquals(false, v.ui.value.working)
        org.junit.Assert.assertNotNull(f.repo.local())
    }

    @Test fun `the only key reason is also shown when the phone was already signed in`() = runTest {
        val f = KeyDeviceFixture(backgroundScope) // signed in with the key: no extra sign-in for the removal
        val v = ConfirmItsYouViewModel(deviceSignIn, f.repo, signOut, SessionEndLog(), probe)
        coEvery { signOut.signOutAndRemovePhone(activity) } returns StepUpOutcome.Failed("LAST_STRONG_AUTHENTICATOR")
        v.removeThisPhone(activity)
        assertEquals(R.string.security_only_key, v.ui.value.error)
        coVerify(exactly = 0) { deviceSignIn.signIn(any(), any()) }
        coVerify(exactly = 0) { signOut.signOut() } // the person's own session stays
        org.junit.Assert.assertNotNull(f.repo.local())
        coEvery { signOut.signOutAndRemovePhone(activity) } returns StepUpOutcome.Cancelled
        v.removeThisPhone(activity)
        // A later attempt clears the old reason.
        assertNull(v.ui.value.error)
    }

    @Test fun `a key forgotten while the screen is open leaves only OK`() = runTest {
        val f = KeyDeviceFixture(backgroundScope)
        f.authState.setLoggedOut()
        val v = ConfirmItsYouViewModel(deviceSignIn, f.repo, signOut, SessionEndLog(), probe)
        assertEquals(false, v.ui.value.keyGone)
        f.repo.forget() // e.g. a setup link opened on top found the key removed
        runCurrent()
        assertTrue(v.ui.value.keyGone)
        assertEquals(R.string.security_device_revoked, v.ui.value.error)
        v.confirm(activity)
        v.removeThisPhone(activity)
        coVerify(exactly = 0) { deviceSignIn.signIn(any(), any()) }
        coVerify(exactly = 0) { signOut.signOutAndRemovePhone(any()) }
        v.acknowledgeKeyGone()
        assertEquals(ConfirmDone.SIGNED_OUT, v.ui.value.done)
    }

    @Test fun `a sign-in that finds no key any more offers OK`() = runTest {
        val v = lapsed()
        coEvery { deviceSignIn.signIn(activity, any()) } returns DeviceSignInResult.Failed("NO_KEY_DEVICE")
        v.confirm(activity)
        assertTrue(v.ui.value.keyGone)
        assertEquals(R.string.security_device_revoked, v.ui.value.error)
    }

    @Test fun `a sign-in made only for a removal does not outlive the app`() = runTest {
        val f = KeyDeviceFixture(backgroundScope)
        f.authState.setLoggedOut()
        val v = ConfirmItsYouViewModel(deviceSignIn, f.repo, signOut, SessionEndLog(), probe)
        coEvery { deviceSignIn.signIn(activity, any()) } answers {
            f.authState.setLoggedIn(io.wolftown.kaiku.testutil.SigFixtures.USER_ID)
            DeviceSignInResult.Success
        }
        // The removal prompt is up when the app is killed: nothing after it runs.
        coEvery { signOut.signOutAndRemovePhone(activity) } coAnswers { kotlinx.coroutines.awaitCancellation() }
        v.removeThisPhone(activity)

        // The next start reads the stored record again: the mark is still there.
        val restarted = io.wolftown.kaiku.data.keydevice.KeyDeviceRepository(f.store, f.keyStore, f.authState, backgroundScope)
        assertTrue(restarted.removalSignInPending())
        org.junit.Assert.assertNotNull(restarted.local())
    }

    @Test fun `a removal stopped while the app stays open signs the temporary session out`() = runTest {
        // Back on the root screen finishes the activity (Android 8 to 11): the ViewModel goes, the process stays.
        val f = KeyDeviceFixture(backgroundScope)
        f.authState.setLoggedOut()
        val v = ConfirmItsYouViewModel(deviceSignIn, f.repo, signOut, SessionEndLog(), probe)
        coEvery { deviceSignIn.signIn(activity, any()) } answers {
            f.authState.setLoggedIn(io.wolftown.kaiku.testutil.SigFixtures.USER_ID)
            DeviceSignInResult.Success
        }
        coEvery { signOut.signOutAndRemovePhone(activity) } coAnswers { kotlinx.coroutines.awaitCancellation() }
        v.removeThisPhone(activity)
        assertTrue(f.repo.removalSignInPending())
        v.viewModelScope.cancel()
        runCurrent()
        coVerify(exactly = 1) { signOut.signOut() }
        assertEquals(false, f.repo.removalSignInPending())
    }

    @Test fun `a removal stopped during its own sign-in still ends the session it made`() = runTest {
        val f = KeyDeviceFixture(backgroundScope)
        f.authState.setLoggedOut()
        val v = ConfirmItsYouViewModel(deviceSignIn, f.repo, signOut, SessionEndLog(), probe)
        coEvery { deviceSignIn.signIn(activity, any()) } coAnswers {
            f.authState.setLoggedIn(io.wolftown.kaiku.testutil.SigFixtures.USER_ID) // the session is stored
            kotlinx.coroutines.awaitCancellation() // and the screen goes before the answer is back
        }
        v.removeThisPhone(activity)
        v.viewModelScope.cancel()
        runCurrent()
        coVerify(exactly = 1) { signOut.signOut() }
        coVerify(exactly = 0) { signOut.signOutAndRemovePhone(any()) }
        assertEquals(false, f.repo.removalSignInPending())
    }

    @Test fun `a key forgotten while the prompt is open leaves only OK`() = runTest {
        val f = KeyDeviceFixture(backgroundScope)
        f.authState.setLoggedOut()
        val v = ConfirmItsYouViewModel(deviceSignIn, f.repo, signOut, SessionEndLog(), probe)
        val prompt = kotlinx.coroutines.CompletableDeferred<Unit>()
        coEvery { deviceSignIn.signIn(activity, any()) } coAnswers {
            prompt.await()
            DeviceSignInResult.Cancelled
        }
        v.confirm(activity)
        assertTrue(v.ui.value.working)
        f.repo.forget() // while the prompt is open, e.g. a setup link on top found the key removed
        runCurrent()
        prompt.complete(Unit) // then the prompt is cancelled
        runCurrent()
        assertEquals(false, v.ui.value.working)
        assertTrue(v.ui.value.keyGone)
        assertEquals(R.string.security_device_revoked, v.ui.value.error)
    }

    @Test fun `a removal that ended leaves nothing to clean up at the next start`() = runTest {
        val f = KeyDeviceFixture(backgroundScope)
        f.authState.setLoggedOut()
        val v = ConfirmItsYouViewModel(deviceSignIn, f.repo, signOut, SessionEndLog(), probe)
        coEvery { deviceSignIn.signIn(activity, any()) } returns DeviceSignInResult.Success
        coEvery { signOut.signOutAndRemovePhone(activity) } returns StepUpOutcome.Cancelled
        v.removeThisPhone(activity)
        val restarted = io.wolftown.kaiku.data.keydevice.KeyDeviceRepository(f.store, f.keyStore, f.authState, backgroundScope)
        assertEquals(false, restarted.removalSignInPending())
    }

    @Test fun `a key the admin removed is noticed on open and the phone is no key any more`() = runTest {
        // No tap needed; the text says what happened and OK leads to the sign-in for a new setup.
        coEvery { probe.probe(any(), any()) } returns io.wolftown.kaiku.data.keydevice.ServerKeyState.Gone
        val v = lapsed()
        assertTrue(v.ui.value.keyGone)
        assertEquals(R.string.security_device_revoked, v.ui.value.error)
        assertNull(v.ui.value.done)
        v.acknowledgeKeyGone()
        assertEquals(ConfirmDone.SIGNED_OUT, v.ui.value.done)
    }

    @Test fun `the probe forgets the key only when the server says it is gone`() = runTest {
        for (st in listOf(
            io.wolftown.kaiku.data.keydevice.ServerKeyState.Active,
            io.wolftown.kaiku.data.keydevice.ServerKeyState.Unknown,
            io.wolftown.kaiku.data.keydevice.ServerKeyState.Pending(Instant.parse("2026-11-20T08:00:00Z")),
        )) {
            coEvery { probe.probe(any(), any()) } returns st
            val f = KeyDeviceFixture(backgroundScope)
            f.authState.setLoggedOut()
            val v = ConfirmItsYouViewModel(deviceSignIn, f.repo, signOut, SessionEndLog(), probe)
            assertEquals(false, v.ui.value.keyGone)
            org.junit.Assert.assertNotNull(st.toString(), f.repo.local())
        }
        coVerify { probe.probe(io.wolftown.kaiku.testutil.SigFixtures.USER_ID, io.wolftown.kaiku.testutil.SigFixtures.ORIGIN) }
    }

    @Test fun `a signed-in phone is not probed`() = runTest {
        val f = KeyDeviceFixture(backgroundScope) // signed in: the renewal screen
        ConfirmItsYouViewModel(deviceSignIn, f.repo, signOut, SessionEndLog(), probe)
        coVerify(exactly = 0) { probe.probe(any(), any()) }
    }

    @Test fun `prompts name their purpose and a slow prompt says so`() = runTest {
        val v = lapsed()
        coEvery { deviceSignIn.signIn(activity, any()) } returns DeviceSignInResult.Expired
        v.confirm(activity)
        coVerify { deviceSignIn.signIn(activity, io.wolftown.kaiku.data.auth.SignInPurpose.SIGN_IN) }
        assertEquals(R.string.security_took_too_long, v.ui.value.error)
        v.confirm(activity, stayingSignedIn = true)
        coVerify { deviceSignIn.signIn(activity, io.wolftown.kaiku.data.auth.SignInPurpose.STAY_SIGNED_IN) }
        coEvery { signOut.signOutAndRemovePhone(activity) } returns StepUpOutcome.Failed("APPROVAL_EXPIRED")
        coEvery { deviceSignIn.signIn(activity, any()) } returns DeviceSignInResult.Success
        v.removeThisPhone(activity)
        coVerify { deviceSignIn.signIn(activity, io.wolftown.kaiku.data.auth.SignInPurpose.FOR_REMOVAL) }
        assertEquals(R.string.security_took_too_long, v.ui.value.error)
    }
}
