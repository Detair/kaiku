package io.wolftown.kaiku.data.auth

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
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
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

    private fun TestScope.setUp(uv: UvResult? = null, loggedIn: Boolean = false): Pair<DeviceSignIn, KeyDeviceRepository> {
        val authState = AuthState(backgroundScope)
        val repo = KeyDeviceRepository(store, keyStore, authState, backgroundScope)
        repo.saveClaimed(
            LocalKeyDevice("kd1", SigFixtures.USER_ID, SigFixtures.ORIGIN, UvClass.PER_USE, "e1", null, "Alice"),
        )
        authState.setLoggedIn(SigFixtures.USER_ID)
        repo.markActive(SigFixtures.USER_ID, "kd1")
        if (!loggedIn) authState.setLoggedOut()
        coEvery { authRepository.beginDeviceSignIn(SigFixtures.ORIGIN) } returns start
        coEvery { authRepository.installDeviceSession(any(), any(), any()) } answers {
            authState.setLoggedIn(SigFixtures.USER_ID)
            Result.success(
                User(SigFixtures.USER_ID, "alice", "Alice", session = SessionInfo("77777777-7777-4777-8777-777777777777", "full", keyDeviceId = "kd1")),
            )
        }
        coEvery { authRepository.hasLiveSession() } returns loggedIn
        val invalidation = io.wolftown.kaiku.data.keydevice.KeyInvalidation(api, repo, authRepository)
        val v = FakeUserVerifier(keyStore, uv).also { verifier = it }
        val signer = DeviceSigner(keyStore, v, checks, invalidation::handle)
        return DeviceSignIn(api, signer, repo, { SigFixtures.DEVICE_ID }, authRepository) to repo
    }

    @Test fun `device sign-in installs a full session`() = runTest {
        val (signIn, repo) = setUp()
        assertEquals(KeyDeviceState.Lapsed(SigFixtures.USER_ID), repo.current())

        assertEquals(DeviceSignInResult.Success, signIn.signIn(activity))

        assertEquals(
            listOf("deviceLoginChallenge:${SigFixtures.USER_ID}:${SigFixtures.DEVICE_ID}", "deviceLogin:${SigFixtures.challengeId(3)}"),
            api.calls,
        )
        coVerify { authRepository.installDeviceSession(api.deviceLoginResult, SigFixtures.USER_ID, start) }
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
        coEvery { authRepository.installDeviceSession(any(), any(), any()) } returns Result.failure(IllegalStateException("superseded"))

        assertEquals(DeviceSignInResult.Failed("SESSION_NOT_INSTALLED"), signIn.signIn(activity))

        assertEquals(KeyDeviceState.Lapsed(SigFixtures.USER_ID), repo.current())
    }
}
