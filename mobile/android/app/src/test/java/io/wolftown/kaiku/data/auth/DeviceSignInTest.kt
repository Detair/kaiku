package io.wolftown.kaiku.data.auth

import io.wolftown.kaiku.data.keydevice.KeyAnswers
import io.wolftown.kaiku.testutil.forget
import io.wolftown.kaiku.approval.keyId
import kotlinx.coroutines.async
import io.wolftown.kaiku.testutil.saveClaimed
import io.wolftown.kaiku.testutil.markActive
import androidx.fragment.app.FragmentActivity
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.mockk
import io.wolftown.kaiku.approval.ChallengeVerifier
import io.wolftown.kaiku.approval.DeviceSigner
import io.wolftown.kaiku.approval.FakeUserVerifier
import io.wolftown.kaiku.approval.SigKind
import io.wolftown.kaiku.approval.SoftwareApprovalKeyStore
import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.approval.UvDecision
import io.wolftown.kaiku.approval.UvPolicy
import io.wolftown.kaiku.approval.UvResult
import io.wolftown.kaiku.data.api.ApiErrorFields
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.KeyDeviceState
import io.wolftown.kaiku.data.keydevice.LocalKeyDevice
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.repository.AuthRepository
import io.wolftown.kaiku.data.repository.DeviceSignInStart
import io.wolftown.kaiku.domain.model.SessionInfo
import io.wolftown.kaiku.domain.model.User
import io.wolftown.kaiku.testutil.FakeApprovalApi
import io.wolftown.kaiku.testutil.InMemoryKeyDeviceStore
import io.wolftown.kaiku.testutil.SigFixtures
import io.wolftown.kaiku.testutil.apiError
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import io.wolftown.kaiku.data.keydevice.KeyDevicePhase
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import io.wolftown.kaiku.testutil.seedAsIs
import org.junit.Test
import java.time.Instant

class DeviceSignInTest {

    private val activity: FragmentActivity = mockk(relaxed = true)
    private val keyStore = SoftwareApprovalKeyStore().apply {
        generate(ByteArray(32), UvPolicy.choose(30, true, true) as UvDecision.Use)
    }
    private val api = FakeApprovalApi().apply {
        nextChallenge = SigFixtures.challengeFor(SigKind.DEVICE_LOGIN, 3, SigFixtures.V03_CONTEXT)
    }
    private val store = InMemoryKeyDeviceStore()
    private val authRepository: AuthRepository = mockk()
    private val checks = ChallengeVerifier({ SigFixtures.ORIGIN }, { SigFixtures.EXPIRES_AT - 30 })
    private val start = DeviceSignInStart(0L, SigFixtures.ORIGIN, io.wolftown.kaiku.data.local.AuthSession.LoggedOut)
    private var verifier: FakeUserVerifier? = null
    /** Runs while the session's profile is read, before it is installed / right after it was. */
    private var beforeInstall: ((KeyDeviceRepository) -> Unit)? = null
    private var afterInstall: ((KeyDeviceRepository) -> Unit)? = null

    private lateinit var authState: AuthState

    private fun TestScope.setUp(uv: UvResult? = null, loggedIn: Boolean = false): Pair<DeviceSignIn, KeyDeviceRepository> {
        authState = AuthState(backgroundScope)
        val repo = KeyDeviceRepository(store, keyStore, authState, backgroundScope)
        repo.saveClaimed(
            LocalKeyDevice("kd1", SigFixtures.USER_ID, SigFixtures.ORIGIN, UvClass.PER_USE, "e1", null, "Alice"),
        )
        authState.setLoggedIn(SigFixtures.USER_ID)
        repo.markActive(SigFixtures.USER_ID, "kd1")
        if (!loggedIn) authState.setLoggedOut()
        coEvery { authRepository.beginDeviceSignIn(SigFixtures.ORIGIN) } returns start
        coEvery { authRepository.installDeviceSession(any(), any(), any(), any(), any()) } coAnswers {
            beforeInstall?.invoke(repo)
            // As the real one: installed only while the key it was made with is still the phone's key.
            if (!arg<() -> Boolean>(3)()) {
                Result.failure(IllegalStateException("superseded"))
            } else {
                authState.setLoggedIn(SigFixtures.USER_ID)
                arg<(io.wolftown.kaiku.data.local.AuthSession.LoggedIn) -> Unit>(4)(authState.session.value as io.wolftown.kaiku.data.local.AuthSession.LoggedIn)
                afterInstall?.invoke(repo)
                Result.success(
                    User(SigFixtures.USER_ID, "alice", "Alice", session = SessionInfo("77777777-7777-4777-8777-777777777777", "full", keyDeviceId = "kd1")),
                )
            }
        }
        coEvery { authRepository.logout() } answers { authState.setLoggedOut() }
        coEvery { authRepository.logoutIfCurrent(any()) } answers {
            if (authState.session.value == firstArg<io.wolftown.kaiku.data.local.AuthSession.LoggedIn>()) authState.setLoggedOut()
        }
        coEvery { authRepository.hasLiveSession() } returns loggedIn
        val invalidation = io.wolftown.kaiku.data.keydevice.KeyInvalidation(api, repo, authRepository)
        val v = FakeUserVerifier(keyStore, uv).also { verifier = it }
        val signer = DeviceSigner(keyStore, v, checks, invalidation::handle)
        return DeviceSignIn(api, signer, repo, { appInstallId }, authRepository, KeyAnswers(repo)) to repo
    }

    /** The install id the app has now. */
    private var appInstallId = SigFixtures.DEVICE_ID

    /** The server lists the key device under the id it was enrolled with: the sign-in names that one. */
    @Test fun `the sign-in names the install id of the record, whatever id the app has now`() = runTest {
        val (signIn, repo) = setUp()
        repo.bindInstallId(SigFixtures.DEVICE_ID)
        appInstallId = "0a0a0a0a-0a0a-4a0a-8a0a-0a0a0a0a0a0a"
        assertEquals(DeviceSignInResult.Success, signIn.signIn(activity))
        assertEquals("deviceLoginChallenge:${SigFixtures.USER_ID}:${SigFixtures.DEVICE_ID}", api.calls.first())
        assertEquals(SigFixtures.DEVICE_ID, repo.record.value!!.installId)
    }

    @Test fun `a sign-in that worked takes back what was noted about a silent server`() = runTest {
        val (signIn, repo) = setUp()
        repo.update(repo.record.value!!) {
            it.copy(unanswered = io.wolftown.kaiku.data.keydevice.Unanswered(io.wolftown.kaiku.data.keydevice.BootTime(1, 90_000_000), 3, java.time.Duration.ofHours(25)))
        }
        assertEquals(DeviceSignInResult.Success, signIn.signIn(activity))
        assertEquals(null, repo.record.value!!.unanswered)
        assertEquals(null, store.stored!!.unanswered)
    }

    /**
     * A record that names no install id (its binding never reached storage): nothing is asked,
     * since an id the app has now may have been made later and the server would honestly say it
     * does not know it. The key stays.
     */
    @Test fun `a record without its install id asks nothing and keeps the key`() = runTest {
        val (signIn, repo) = setUp()
        repo.seedAsIs(repo.record.value!!.copy(installId = null))
        assertEquals(DeviceSignInResult.Failed("INSTALL_ID_LOST"), signIn.signIn(activity))
        assertTrue(api.calls.isEmpty())
        assertTrue(keyStore.hasKey())
        assertEquals(KeyDevicePhase.ACTIVE, repo.phase())
    }

    @Test fun `device sign-in installs a full session`() = runTest {
        val (signIn, repo) = setUp()
        assertEquals(KeyDeviceState.Lapsed(SigFixtures.USER_ID), repo.current())

        assertEquals(DeviceSignInResult.Success, signIn.signIn(activity))

        assertEquals(
            listOf("deviceLoginChallenge:${SigFixtures.USER_ID}:${SigFixtures.DEVICE_ID}", "deviceLogin:${SigFixtures.challengeId(3)}"),
            api.calls,
        )
        coVerify { authRepository.installDeviceSession(api.deviceLoginResult, SigFixtures.USER_ID, start, any(), any()) }
        assertEquals(KeyDeviceState.Active(SigFixtures.USER_ID), repo.current())
        assertEquals(Instant.parse("2027-02-01T12:00:00Z"), repo.sessionCap())
    }

    @Test fun `revoked device forgets the key`() = runTest {
        val (signIn, repo) = setUp()
        api.failWith = apiError(403, "KEY_DEVICE_REVOKED")
        assertEquals(DeviceSignInResult.Revoked, signIn.signIn(activity))
        assertEquals(KeyDeviceState.None, repo.current())
        assertFalse(keyStore.hasKey())

        api.failWith = apiError(404, "KEY_DEVICE_UNKNOWN")
        assertTrue(signIn.signIn(activity) is DeviceSignInResult.Failed)
    }

    @Test fun `a removal code with another status than the server's own keeps the key`() = runTest {
        val (signIn, repo) = setUp()
        for (x in listOf(apiError(500, "KEY_DEVICE_REVOKED"), apiError(503, "KEY_DEVICE_UNKNOWN"), apiError(404, "KEY_DEVICE_REVOKED"))) {
            api.failWith = x
            val r = signIn.signIn(activity)
            assertTrue(r.toString(), r is DeviceSignInResult.Failed)
            assertTrue(keyStore.hasKey())
            assertEquals(KeyDeviceState.Lapsed(SigFixtures.USER_ID), repo.current())
        }
    }

    @Test fun `pending device returns ready at`() = runTest {
        val (signIn, repo) = setUp()
        api.failWith = apiError(409, "KEY_DEVICE_PENDING", ApiErrorFields(readyAt = "2026-11-04T12:00:00Z"))
        assertEquals(DeviceSignInResult.Pending(Instant.parse("2026-11-04T12:00:00Z")), signIn.signIn(activity))
        assertEquals(KeyDeviceState.Pending(Instant.parse("2026-11-04T12:00:00Z"), "e1"), repo.current())
        assertTrue(keyStore.hasKey())
    }

    @Test fun `banned account and network errors change nothing`() = runTest {
        val (signIn, repo) = setUp()
        api.failWith = apiError(403, "ACCOUNT_BANNED")
        assertEquals(DeviceSignInResult.Banned, signIn.signIn(activity))
        api.failWith = java.io.IOException("offline")
        assertEquals(DeviceSignInResult.Network, signIn.signIn(activity))
        // A captive portal's page or another non-IO failure: no crash, nothing forgotten.
        api.failWith = kotlinx.serialization.SerializationException("<html>")
        assertEquals(DeviceSignInResult.Network, signIn.signIn(activity))
        assertEquals(KeyDeviceState.Lapsed(SigFixtures.USER_ID), repo.current())
    }

    @Test fun `invalidated key reports and forgets the device`() = runTest {
        val (signIn, repo) = setUp(loggedIn = true)
        keyStore.invalidated = true
        assertEquals(DeviceSignInResult.KeyInvalidated, signIn.signIn(activity))
        assertTrue("reportInvalidated:kd1" in api.calls)
        assertEquals(KeyDeviceState.None, repo.current())
        assertFalse(keyStore.hasKey())
    }

    @Test fun `invalidated key without a session just forgets`() = runTest {
        val (signIn, repo) = setUp(loggedIn = false)
        keyStore.invalidated = true
        assertEquals(DeviceSignInResult.KeyInvalidated, signIn.signIn(activity))
        assertFalse(api.calls.any { it.startsWith("reportInvalidated") })
        assertEquals(KeyDeviceState.None, repo.current())
    }

    @Test fun `cancelled prompt changes nothing`() = runTest {
        val (signIn, repo) = setUp(uv = UvResult.Cancelled)
        assertEquals(DeviceSignInResult.Cancelled, signIn.signIn(activity))
        assertFalse(api.calls.any { it.startsWith("deviceLogin:") })
        assertEquals(KeyDeviceState.Lapsed(SigFixtures.USER_ID), repo.current())
        assertTrue(keyStore.hasKey())
    }

    @Test fun `a sign-in challenge for another client class is refused before the prompt`() = runTest {
        val (signIn, repo) = setUp()
        api.nextChallenge = SigFixtures.challengeFor(SigKind.DEVICE_LOGIN, 3, """{"client_class":"web"}""")

        assertEquals(DeviceSignInResult.Failed("CHALLENGE_CONTEXT_MISMATCH"), signIn.signIn(activity))

        assertTrue(verifier!!.prompts.isEmpty())
        assertFalse(api.calls.any { it.startsWith("deviceLogin:") })
        assertEquals(KeyDeviceState.Lapsed(SigFixtures.USER_ID), repo.current())
    }

    @Test fun `a running session on another server stops the sign-in before any request`() = runTest {
        val (signIn, _) = setUp()
        coEvery { authRepository.beginDeviceSignIn(SigFixtures.ORIGIN) } returns null

        assertEquals(DeviceSignInResult.Failed("OTHER_SERVER"), signIn.signIn(activity))

        assertTrue(api.calls.isEmpty())
    }

    @Test fun `a session the repository does not install is not marked active`() = runTest {
        val (signIn, repo) = setUp()
        coEvery { authRepository.installDeviceSession(any(), any(), any(), any(), any()) } returns Result.failure(IllegalStateException("superseded"))

        assertEquals(DeviceSignInResult.Failed("SESSION_NOT_INSTALLED"), signIn.signIn(activity))

        assertEquals(KeyDeviceState.Lapsed(SigFixtures.USER_ID), repo.current())
    }

    @Test fun `a challenge that ran out during a slow prompt says so and keeps the key`() = runTest {
        // The device_login challenge lives 120 s; a prompt left open longer is answered 410.
        val (signIn, repo) = setUp()
        api.deviceLoginFailure = apiError(410, "APPROVAL_EXPIRED")
        assertEquals(DeviceSignInResult.Expired, signIn.signIn(activity))
        assertTrue(repo.local() != null)
        // Trying again asks the server for a fresh challenge.
        api.deviceLoginFailure = null
        assertEquals(DeviceSignInResult.Success, signIn.signIn(activity))
        assertEquals(2, api.calls.count { it.startsWith("deviceLoginChallenge:") })
    }

    @Test fun `the prompt says what the sign-in is for`() = runTest {
        // The screen-lock prompt names its purpose.
        io.mockk.every { activity.getString(io.wolftown.kaiku.R.string.security_uv_sign_in) } returns "sign in"
        io.mockk.every { activity.getString(io.wolftown.kaiku.R.string.security_uv_after_setup) } returns "after setup"
        io.mockk.every { activity.getString(io.wolftown.kaiku.R.string.security_uv_after_setup_detail) } returns "after setup detail"
        io.mockk.every { activity.getString(io.wolftown.kaiku.R.string.security_uv_stay_signed_in) } returns "stay"
        val expected = mapOf(
            SignInPurpose.SIGN_IN to ("sign in" to null),
            SignInPurpose.AFTER_SETUP to ("after setup" to "after setup detail"),
            SignInPurpose.STAY_SIGNED_IN to ("stay" to null),
        )
        for ((purpose, text) in expected) {
            val (signIn, _) = setUp()
            assertEquals(DeviceSignInResult.Success, signIn.signIn(activity, purpose))
            val prompt = verifier!!.prompts.single()
            assertEquals(io.wolftown.kaiku.R.string.security_uv_title, prompt.title)
            assertEquals(purpose.name, text.first, prompt.subtitle)
            assertEquals(purpose.name, text.second, prompt.description)
        }
    }

    @Test fun `both requests go to the server of the key, whatever server is configured`() = runTest {
        val (signIn, _) = setUp()
        assertEquals(DeviceSignInResult.Success, signIn.signIn(activity))
        assertEquals(SigFixtures.ORIGIN, api.deviceLoginOrigin)
        assertEquals(SigFixtures.ORIGIN, api.deviceLoginFinishOrigin)
    }

    @Test fun `a key forgotten while the sign-in finishes is never reported as signed in`() = runTest {
        val (signIn, repo) = setUp()
        beforeInstall = { it.forget() } // e.g. its server said it was removed, on another screen
        assertEquals(DeviceSignInResult.Failed("NO_KEY_DEVICE"), signIn.signIn(activity))
        assertEquals(KeyDeviceState.None, repo.current())
    }

    @Test fun `a key forgotten right after the session was installed ends that session`() = runTest {
        val (signIn, repo) = setUp()
        afterInstall = { it.forget() }
        assertEquals(DeviceSignInResult.Failed("NO_KEY_DEVICE"), signIn.signIn(activity))
        coVerify(exactly = 1) { authRepository.logoutIfCurrent(any()) }
        assertEquals(KeyDeviceState.None, repo.current())
    }

    /**
     * The sign-in installed its session; before it could note that on the key record, the key was
     * forgotten and another account signed in. What it ends is only the session it installed
     * itself: the newer one stays.
     */
    @Test fun `a late clean-up never ends a newer session`() = runTest {
        val (signIn, repo) = setUp()
        afterInstall = {
            it.forget()
            authState.setLoggedOut()
            authState.setLoggedIn("someone-else")
        }
        assertEquals(DeviceSignInResult.Failed("NO_KEY_DEVICE"), signIn.signIn(activity))
        coVerify(exactly = 0) { authRepository.logout() }
        assertEquals("someone-else", (authState.session.value as io.wolftown.kaiku.data.local.AuthSession.LoggedIn).userId)
    }

    @Test fun `a removal answer about the key the sign-in started with never costs a key made meanwhile`() = runTest {
        val (signIn, repo) = setUp()
        api.challengeGate = kotlinx.coroutines.CompletableDeferred()
        api.failWith = apiError(404, "KEY_DEVICE_UNKNOWN")
        val job = async { signIn.signIn(activity) }
        testScheduler.runCurrent()
        // Meanwhile the phone was set up again: another key, another record.
        repo.forget()
        val newKey = keyStore.generate(ByteArray(32), UvPolicy.choose(30, true, true) as UvDecision.Use).keyId
        repo.saveClaimed(LocalKeyDevice(null, SigFixtures.USER_ID, SigFixtures.ORIGIN, UvClass.PER_USE, "e2", "secret"))
        api.challengeGate!!.complete(Unit)
        assertEquals(DeviceSignInResult.Failed("NO_KEY_DEVICE"), job.await())
        assertEquals("e2", repo.local()!!.enrollmentId)
        assertEquals(newKey, keyStore.keyId())
    }

    @Test fun `only an active key signs in`() = runTest {
        val (signIn, repo) = setUp()
        repo.saveClaimed(LocalKeyDevice(null, SigFixtures.USER_ID, SigFixtures.ORIGIN, UvClass.PER_USE, "e1", "secret"))
        assertEquals(DeviceSignInResult.Failed("NO_KEY_DEVICE"), signIn.signIn(activity))
        assertTrue(api.calls.isEmpty())
    }

    /**
     * A record that took the probe's word ("this install is a key device") only provisionally: the
     * server refuses this key's signature, so that word was about another key. The record goes
     * back to the claim it was, is marked so, and keeps its key; nothing is installed.
     */
    @Test fun `a provisional key device whose signature is refused goes back to its claim`() = runTest {
        val (signIn, repo) = setUp()
        val rec = repo.record.value!!
        repo.update(rec) { it.copy(provisionalFrom = KeyDevicePhase.CLAIMING) }
        api.deviceLoginFailure = apiError(400, "SIGNATURE_INVALID")
        assertEquals(DeviceSignInResult.Failed("NOT_THIS_KEY"), signIn.signIn(activity))
        val back = repo.record.value!!
        assertEquals(KeyDevicePhase.CLAIMING, back.phase)
        assertNull(back.provisionalFrom)
        assertTrue(back.notThisKey)
        assertTrue(keyStore.hasKey())
        coVerify(exactly = 0) { authRepository.installDeviceSession(any(), any(), any(), any(), any()) }
    }

    /** The same refusal for a key device that is one for good changes nothing about the record. */
    @Test fun `a refused signature of a key device for good keeps it as it is`() = runTest {
        val (signIn, repo) = setUp()
        api.deviceLoginFailure = apiError(400, "SIGNATURE_INVALID")
        signIn.signIn(activity)
        assertEquals(KeyDevicePhase.ACTIVE, repo.phase())
        assertFalse(repo.record.value!!.notThisKey)
    }

    /** A key whose install id is lost is kept on "no key device", and the sign-in says so. */
    @Test fun `a key whose install id is lost is kept on no key device`() = runTest {
        val (signIn, repo) = setUp()
        repo.update(repo.record.value!!) { it.copy(installIdLost = true) }
        api.failWith = apiError(404, "KEY_DEVICE_UNKNOWN")
        assertEquals(DeviceSignInResult.Failed("INSTALL_ID_LOST"), signIn.signIn(activity))
        assertEquals(KeyDevicePhase.ACTIVE, repo.phase())
        assertTrue(keyStore.hasKey())
    }

    /** The record could not be sent back (the write failed): no "different key" claim, a plain failure. */
    @Test fun `a revert that did not happen is not reported as one`() = runTest {
        val (signIn, repo) = setUp()
        repo.update(repo.record.value!!) { it.copy(provisionalFrom = KeyDevicePhase.CLAIMING) }
        api.deviceLoginFailure = apiError(400, "SIGNATURE_INVALID")
        store.failWrites = true
        assertEquals(DeviceSignInResult.Failed("SIGNATURE_INVALID"), signIn.signIn(activity))
        assertEquals(KeyDevicePhase.ACTIVE, repo.phase())
    }

    /**
     * A provisional key device whose sign-in the server accepted, but the note that it is a key
     * device for good cannot be written: the sign-in does not report success; it says that saving
     * failed. The session stays installed, and the record is still provisional.
     */
    @Test fun `a provisional key whose note cannot be written says that saving failed`() = runTest {
        val (signIn, repo) = setUp()
        val rec = repo.record.value!!
        repo.update(rec) { it.copy(provisionalFrom = KeyDevicePhase.CLAIMED) }
        afterInstall = { store.failWrites = true }
        val r = signIn.signIn(activity)
        assertEquals(DeviceSignInResult.Failed("NOT_SAVED"), r)
        assertEquals(io.wolftown.kaiku.R.string.security_key_storage_not_saved, io.wolftown.kaiku.ui.auth.signInResultText(r))
        // The setup's final step says the same and offers to try again.
        assertEquals(
            io.wolftown.kaiku.ui.auth.setup.SetupUi.Ended(io.wolftown.kaiku.R.string.security_key_storage_not_saved, canRetry = true),
            io.wolftown.kaiku.ui.auth.setup.finalSignInUi(r),
        )
        assertEquals(KeyDevicePhase.CLAIMED, repo.record.value!!.provisionalFrom)
        assertTrue(authState.session.value is io.wolftown.kaiku.data.local.AuthSession.LoggedIn)
    }

    /** A key device for good whose note beside the record cannot be written: the sign-in stands. */
    @Test fun `a key device for good whose note cannot be written still signs in`() = runTest {
        val (signIn, _) = setUp()
        afterInstall = { store.failWrites = true }
        assertEquals(DeviceSignInResult.Success, signIn.signIn(activity))
    }
}
