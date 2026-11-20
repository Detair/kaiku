package io.wolftown.kaiku.ui.auth

import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.viewModelScope
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.mockk
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.StepUpOutcome
import io.wolftown.kaiku.data.auth.DeviceSignIn
import io.wolftown.kaiku.data.auth.DeviceSignInResult
import io.wolftown.kaiku.data.auth.KeyDeviceSignOut
import io.wolftown.kaiku.data.auth.RemovalResult
import io.wolftown.kaiku.data.auth.SessionEndLog
import io.wolftown.kaiku.data.keydevice.KeyDeviceProbe
import io.wolftown.kaiku.data.keydevice.ServerAnswer
import io.wolftown.kaiku.data.keydevice.ServerKeyState
import io.wolftown.kaiku.testutil.KeyDeviceFixture
import io.wolftown.kaiku.testutil.SigFixtures
import io.wolftown.kaiku.testutil.forget
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.cancel
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.time.Instant

/** "Confirm it's you" and "Use another account" on a lapsed key phone. */
@OptIn(ExperimentalCoroutinesApi::class)
class ConfirmItsYouViewModelTest {
    private val activity: FragmentActivity = mockk(relaxed = true)
    private val deviceSignIn: DeviceSignIn = mockk()
    private val signOut: KeyDeviceSignOut = mockk(relaxed = true)
    private val probe: KeyDeviceProbe = mockk {
        coEvery { probe(any()) } returns ServerKeyState.Active
    }
    private val gone = ServerKeyState.Gone(ServerAnswer(404, "KEY_DEVICE_UNKNOWN"))

    @Before fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())
    @After fun tearDown() = Dispatchers.resetMain()

    /** The phone's own clock. */
    private val base: Instant = Instant.parse("2026-11-20T09:00:00Z")
    /** The phone's running time; [now] moves it (the tests below speak in clock times). */
    private val clock = io.wolftown.kaiku.testutil.FakeBootClock(boot = 1, elapsedMs = 0)
    private var now: Instant
        get() = base.plusMillis(clock.elapsedMs)
        set(v) {
            clock.elapsedMs = java.time.Duration.between(base, v).toMillis()
        }

    private fun vm(f: KeyDeviceFixture) = ConfirmItsYouViewModel(
        deviceSignIn, f.repo, signOut, SessionEndLog(), probe, f.answers, io.wolftown.kaiku.data.keydevice.ServerSilence(f.repo, clock),
    )

    /**
     * An active key whose server is gone for good can do nothing on this screen: once the server
     * has not answered on several separate occasions over at least a day, the screen names the last
     * resort in words. One open without a connection says nothing of the kind, and nothing here
     * ever deletes the key.
     */
    @Test fun `a server that stays silent for long is named with the last resort, and the key stays`() = runTest {
        coEvery { probe.probe(any()) } returns ServerKeyState.Unknown
        val f = lapsedFixture()
        // Opened once without a connection, long after the last sign-in.
        now = now.plusSeconds(60L * 24 * 3600)
        assertEquals(false, vm(f).ui.value.serverSilent)
        // Again the same evening, and the next morning.
        now = now.plusSeconds(600)
        assertEquals(false, vm(f).ui.value.serverSilent)
        now = now.plusSeconds(12 * 3600)
        assertEquals(false, vm(f).ui.value.serverSilent)
        // A third occasion, more than a day after the first.
        now = now.plusSeconds(13 * 3600)
        val v = vm(f)
        assertTrue(v.ui.value.serverSilent)
        assertEquals(false, v.ui.value.keyGone)
        assertNull(v.ui.value.error)
        assertNotNull(f.repo.local())
        assertTrue(f.keyStore.hasKey())
        assertEquals(3, f.repo.record.value!!.unanswered!!.occasions)
    }

    @Test fun `any answer of the server takes the note back, a refusal too`() = runTest {
        val f = lapsedFixture()
        for (answer in listOf(ServerKeyState.Active, ServerKeyState.Refused, ServerKeyState.Pending(Instant.parse("2026-12-01T08:00:00Z")))) {
            coEvery { probe.probe(any()) } returns ServerKeyState.Unknown
            for (hours in listOf(0L, 12, 13)) {
                now = now.plusSeconds(hours * 3600)
                vm(f)
            }
            assertTrue(answer.toString(), vm(f).ui.value.serverSilent)
            coEvery { probe.probe(any()) } returns answer
            assertEquals(answer.toString(), false, vm(f).ui.value.serverSilent)
            assertNull(answer.toString(), f.repo.record.value!!.unanswered)
            // Silent once more: one occasion, counted anew.
            coEvery { probe.probe(any()) } returns ServerKeyState.Unknown
            now = now.plusSeconds(2 * 3600)
            assertEquals(answer.toString(), false, vm(f).ui.value.serverSilent)
            assertEquals(answer.toString(), 1, f.repo.record.value!!.unanswered!!.occasions)
            now = now.plusSeconds(3600)
            f.repo.update(f.repo.record.value!!) { it.copy(unanswered = null) }
        }
        assertTrue(f.keyStore.hasKey())
    }

    private fun TestScope.lapsedFixture(): KeyDeviceFixture =
        KeyDeviceFixture(backgroundScope).also { it.authState.setLoggedOut() } // an active key without its session: lapsed

    private fun TestScope.lapsed(): ConfirmItsYouViewModel = vm(lapsedFixture())

    @Test fun `a signed-out phone removes itself without the app ever signing in`() = runTest {
        val f = lapsedFixture()
        val v = vm(f)
        coEvery { signOut.removeSignedOutPhone(activity) } returns RemovalResult.Cancelled
        v.removeThisPhone(activity)
        // No device-key sign-in of the app, no sign-out of anything: the removal is on its own.
        coVerify(exactly = 0) { deviceSignIn.signIn(any(), any()) }
        coVerify(exactly = 0) { signOut.signOut() }
        coVerify(exactly = 0) { signOut.signOutAndRemovePhone(any()) }
        assertNull(v.ui.value.done)
        assertNull(v.ui.value.error)
        assertEquals(false, v.ui.value.working)
        assertEquals(io.wolftown.kaiku.data.local.AuthSession.LoggedOut, f.authState.session.value)
    }

    @Test fun `a completed removal signs out for good`() = runTest {
        val v = lapsed()
        coEvery { signOut.removeSignedOutPhone(activity) } returns RemovalResult.Removed
        v.removeThisPhone(activity)
        assertEquals(ConfirmDone.SIGNED_OUT, v.ui.value.done)
        coVerify(exactly = 0) { signOut.signOut() }
    }

    @Test fun `an invalidated key during the removal says so and offers OK`() = runTest {
        val v = lapsed()
        coEvery { signOut.removeSignedOutPhone(activity) } returns RemovalResult.KeyGone(invalidated = true)
        v.removeThisPhone(activity)
        assertTrue(v.ui.value.keyGone)
        assertEquals(R.string.security_key_invalidated, v.ui.value.error)
    }

    @Test fun `a phone the server no longer knows as a key says so and offers OK`() = runTest {
        val v = lapsed()
        coEvery { signOut.removeSignedOutPhone(activity) } returns RemovalResult.KeyGone(invalidated = false)
        v.removeThisPhone(activity)
        assertTrue(v.ui.value.keyGone)
        assertEquals(R.string.security_device_revoked, v.ui.value.error)
    }

    @Test fun `the only key of the account is kept and the reason is shown`() = runTest {
        // The server refuses to remove the last strong authenticator (409 LAST_STRONG_AUTHENTICATOR).
        val f = lapsedFixture()
        val v = vm(f)
        coEvery { signOut.removeSignedOutPhone(activity) } returns RemovalResult.Failed("LAST_STRONG_AUTHENTICATOR")
        v.removeThisPhone(activity)
        assertEquals(R.string.security_only_key, v.ui.value.error)
        assertEquals(false, v.ui.value.keyGone)
        assertNull(v.ui.value.done)
        assertEquals(false, v.ui.value.working)
        assertNotNull(f.repo.local())
    }

    @Test fun `the reasons a removal did not happen have their texts`() = runTest {
        for ((code, text) in listOf(
            "NETWORK" to R.string.security_network_error,
            "UV_LOCKED_OUT" to R.string.security_locked_out,
            "APPROVAL_EXPIRED" to R.string.security_took_too_long,
            "ACCOUNT_BANNED" to R.string.auth_login_banned,
            "INVALID_TOKEN" to R.string.security_sign_in_failed,
        )) {
            val v = lapsed()
            coEvery { signOut.removeSignedOutPhone(activity) } returns RemovalResult.Failed(code)
            v.removeThisPhone(activity)
            assertEquals(code, text, v.ui.value.error)
            assertEquals(code, false, v.ui.value.keyGone)
        }
    }

    @Test fun `the only key reason is also shown when the phone was already signed in`() = runTest {
        val f = KeyDeviceFixture(backgroundScope) // signed in with the key: the person's own session removes
        val v = vm(f)
        coEvery { signOut.signOutAndRemovePhone(activity) } returns StepUpOutcome.Failed("LAST_STRONG_AUTHENTICATOR")
        v.removeThisPhone(activity)
        assertEquals(R.string.security_only_key, v.ui.value.error)
        coVerify(exactly = 0) { signOut.removeSignedOutPhone(any()) }
        coVerify(exactly = 0) { signOut.signOut() } // the person's own session stays
        assertNotNull(f.repo.local())
        coEvery { signOut.signOutAndRemovePhone(activity) } returns StepUpOutcome.Cancelled
        v.removeThisPhone(activity)
        // A later attempt clears the old reason.
        assertNull(v.ui.value.error)
    }

    @Test fun `a removal stopped half way stops there and touches nothing else`() = runTest {
        // Back on the root screen finishes the activity (Android 8 to 11): the ViewModel goes, the process stays.
        val f = lapsedFixture()
        val v = vm(f)
        var stopped = false
        coEvery { signOut.removeSignedOutPhone(activity) } coAnswers {
            try {
                kotlinx.coroutines.awaitCancellation()
            } finally {
                stopped = true
            }
        }
        v.removeThisPhone(activity)
        assertTrue(v.ui.value.working)
        v.viewModelScope.cancel()
        runCurrent()
        assertTrue(stopped)
        coVerify(exactly = 0) { signOut.signOut() }
        coVerify(exactly = 0) { deviceSignIn.signIn(any(), any()) }
        assertNotNull(f.repo.local())
        // A new start reads the same record: nothing was written for the removal.
        val restarted = io.wolftown.kaiku.data.keydevice.KeyDeviceRepository(f.store, f.keyStore, f.authState, backgroundScope)
        assertEquals(f.repo.record.value, restarted.record.value)
        assertTrue(f.store.writes.isEmpty())
    }

    @Test fun `a removal that never ends is given up after its limit`() = runTest {
        val v = lapsed()
        coEvery { signOut.removeSignedOutPhone(activity) } coAnswers { kotlinx.coroutines.awaitCancellation() }
        v.removeThisPhone(activity)
        advanceTimeBy(ConfirmItsYouViewModel.REMOVAL_TIMEOUT_MS - 1)
        assertTrue(v.ui.value.working)
        advanceTimeBy(2)
        assertEquals(false, v.ui.value.working)
        assertEquals(R.string.security_took_too_long, v.ui.value.error)
    }

    /**
     * A sign-in whose prompt never answers (it could not be shown, or its screen went away) does not
     * keep the screen busy for good: after its limit (180 s) the person can try again.
     */
    @Test fun `a sign-in that never ends is given up after its limit`() = runTest {
        val v = lapsed()
        coEvery { deviceSignIn.signIn(any(), any()) } coAnswers { kotlinx.coroutines.awaitCancellation() }
        v.confirm(activity)
        advanceTimeBy(180_000L - 1)
        assertTrue(v.ui.value.working)
        advanceTimeBy(2)
        assertEquals(false, v.ui.value.working)
        assertEquals(R.string.security_took_too_long, v.ui.value.error)
        // The person can try again.
        coEvery { deviceSignIn.signIn(any(), any()) } returns DeviceSignInResult.Cancelled
        v.confirm(activity)
        coVerify(exactly = 2) { deviceSignIn.signIn(any(), any()) }
        assertEquals(false, v.ui.value.working)
    }

    @Test fun `a key forgotten while the screen is open leaves only OK`() = runTest {
        val f = lapsedFixture()
        val v = vm(f)
        assertEquals(false, v.ui.value.keyGone)
        f.repo.forget() // e.g. a setup link opened on top found the key removed
        runCurrent()
        assertTrue(v.ui.value.keyGone)
        assertEquals(R.string.security_device_revoked, v.ui.value.error)
        v.confirm(activity)
        v.removeThisPhone(activity)
        coVerify(exactly = 0) { deviceSignIn.signIn(any(), any()) }
        coVerify(exactly = 0) { signOut.signOutAndRemovePhone(any()) }
        coVerify(exactly = 0) { signOut.removeSignedOutPhone(any()) }
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

    @Test fun `a key forgotten while the prompt is open leaves only OK`() = runTest {
        val f = lapsedFixture()
        val v = vm(f)
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

    @Test fun `a key the admin removed is noticed on open and the phone is no key any more`() = runTest {
        // No tap needed; the text says what happened and OK leads to the sign-in for a new setup.
        coEvery { probe.probe(any()) } returns gone
        val f = lapsedFixture()
        val v = vm(f)
        assertTrue(v.ui.value.keyGone)
        assertEquals(R.string.security_device_revoked, v.ui.value.error)
        assertNull(v.ui.value.done)
        assertNull(f.repo.local())
        assertEquals(false, f.keyStore.hasKey())
        v.acknowledgeKeyGone()
        assertEquals(ConfirmDone.SIGNED_OUT, v.ui.value.done)
    }

    @Test fun `the probe forgets the key only when the server says it is gone`() = runTest {
        for (st in listOf(
            ServerKeyState.Active,
            ServerKeyState.Unknown,
            ServerKeyState.Pending(Instant.parse("2026-11-20T08:00:00Z")),
            ServerKeyState.Refused,
            // "Gone" in words only: not one of the server's two answers.
            ServerKeyState.Gone(ServerAnswer(500, "KEY_DEVICE_UNKNOWN")),
        )) {
            coEvery { probe.probe(any()) } returns st
            val f = lapsedFixture()
            val v = vm(f)
            assertEquals(false, v.ui.value.keyGone)
            assertNotNull(st.toString(), f.repo.local())
        }
        coVerify { probe.probe(match { it.device.userId == SigFixtures.USER_ID && it.device.origin == SigFixtures.ORIGIN }) }
    }

    @Test fun `a probe answer about a key that was replaced meanwhile forgets nothing`() = runTest {
        val f = lapsedFixture()
        val held = kotlinx.coroutines.CompletableDeferred<Unit>()
        coEvery { probe.probe(any()) } coAnswers {
            held.await()
            gone
        }
        val v = vm(f)
        // Meanwhile this phone was set up anew (another setup, same account).
        val now = f.repo.record.value!!
        f.repo.forget()
        f.keyStore.generate(ByteArray(32), io.wolftown.kaiku.approval.UvPolicy.choose(30, true, true) as io.wolftown.kaiku.approval.UvDecision.Use)
        f.store.stored = now.copy(keyId = null, attemptId = "newer")
        f.repo.reload()
        held.complete(Unit)
        runCurrent()
        assertNotNull(f.repo.local())
        assertTrue(f.keyStore.hasKey())
        assertNotNull(v)
    }

    @Test fun `a signed-in phone is not probed`() = runTest {
        val f = KeyDeviceFixture(backgroundScope) // signed in: the renewal screen
        vm(f)
        coVerify(exactly = 0) { probe.probe(any()) }
    }

    @Test fun `prompts name their purpose and a slow prompt says so`() = runTest {
        val v = lapsed()
        coEvery { deviceSignIn.signIn(activity, any()) } returns DeviceSignInResult.Expired
        v.confirm(activity)
        coVerify { deviceSignIn.signIn(activity, io.wolftown.kaiku.data.auth.SignInPurpose.SIGN_IN) }
        assertEquals(R.string.security_took_too_long, v.ui.value.error)
        v.confirm(activity, stayingSignedIn = true)
        coVerify { deviceSignIn.signIn(activity, io.wolftown.kaiku.data.auth.SignInPurpose.STAY_SIGNED_IN) }
    }

    @Test fun `a successful sign-in leads home, a pending key shows when it is ready`() = runTest {
        val v = lapsed()
        coEvery { deviceSignIn.signIn(activity, any()) } returns DeviceSignInResult.Success
        v.confirm(activity)
        assertEquals(ConfirmDone.SIGNED_IN, v.ui.value.done)
        val at = Instant.parse("2026-11-05T08:00:00Z")
        val w = lapsed()
        coEvery { deviceSignIn.signIn(activity, any()) } returns DeviceSignInResult.Pending(at)
        w.confirm(activity)
        assertEquals(ConfirmDone.PENDING, w.ui.value.done)
        assertEquals(at, w.ui.value.readyAt)
    }

    /** The two sign-in outcomes that keep the key say what happened and what the person can do. */
    @Test fun `a refused other key and a lost install id each have their own text`() {
        assertEquals(R.string.setup_not_this_key, signInResultText(DeviceSignInResult.Failed("NOT_THIS_KEY")))
        assertEquals(R.string.confirm_install_unknown, signInResultText(DeviceSignInResult.Failed("INSTALL_ID_LOST")))
        assertEquals(R.string.security_sign_in_failed, signInResultText(DeviceSignInResult.Failed("SOMETHING_ELSE")))
        // At the end of a setup the first one is asked again, not left as a dead end.
        assertEquals(
            io.wolftown.kaiku.ui.auth.setup.SetupUi.Ended(R.string.setup_not_this_key, canRetry = true),
            io.wolftown.kaiku.ui.auth.setup.finalSignInUi(DeviceSignInResult.Failed("NOT_THIS_KEY")),
        )
    }
}
