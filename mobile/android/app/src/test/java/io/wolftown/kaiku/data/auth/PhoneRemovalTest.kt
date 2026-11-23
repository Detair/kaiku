package io.wolftown.kaiku.data.auth

import androidx.fragment.app.FragmentActivity
import io.mockk.coEvery
import io.mockk.mockk
import io.wolftown.kaiku.approval.SigKind
import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.approval.UvDecision
import io.wolftown.kaiku.approval.UvPolicy
import io.wolftown.kaiku.approval.UvResult
import io.wolftown.kaiku.approval.keyId
import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.data.api.RemovalApi
import io.wolftown.kaiku.data.keydevice.KeyDevicePhase
import io.wolftown.kaiku.data.keydevice.LocalKeyDevice
import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.domain.model.AnswerResult
import io.wolftown.kaiku.domain.model.ApprovalChallenge
import io.wolftown.kaiku.domain.model.KeyDevice
import io.wolftown.kaiku.domain.model.KeyDeviceStatus
import io.wolftown.kaiku.domain.model.SessionInfo
import io.wolftown.kaiku.domain.model.StepUpGrant
import io.wolftown.kaiku.domain.model.User
import io.wolftown.kaiku.testutil.FakeApprovalApi
import io.wolftown.kaiku.testutil.InMemorySharedPreferences
import io.wolftown.kaiku.testutil.KeyDeviceFixture
import io.wolftown.kaiku.testutil.SigFixtures
import io.wolftown.kaiku.testutil.apiError
import io.wolftown.kaiku.testutil.forget
import io.wolftown.kaiku.testutil.saveClaimed
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.async
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.IOException

/**
 * "Use another account" on a signed-out key phone. The session it needs is its own: no token of
 * it is ever written to storage, the app is never signed in by it, and however it ends there is
 * nothing to clean up.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class PhoneRemovalTest {

    private companion object {
        const val SID = "77777777-7777-4777-8777-777777777777"
        const val KEY_DEVICE = "99999999-9999-4999-8999-999999999999"
        const val REMOVAL_ACCESS = "new-access" // FakeApprovalApi.deviceLoginResult
        const val REMOVAL_REFRESH = "new-refresh"
    }

    private val activity: FragmentActivity = mockk(relaxed = true)

    /** The explicit-credential calls, recorded with the server and bearer each one named. */
    private class FakeRemovalApi : RemovalApi {
        val calls = mutableListOf<String>()
        val servers = mutableSetOf<String>()
        val bearers = mutableSetOf<String>()
        var me = User(SigFixtures.USER_ID, "alice", "Alice", session = SessionInfo(SID, "full", keyDeviceId = KEY_DEVICE))
        var devices: List<KeyDevice> = emptyList()
        var stepUpChallenge: ApprovalChallenge = SigFixtures.challengeFor(SigKind.DEVICE_STEPUP, 4, SigFixtures.V04_CONTEXT)
        var answerResult = AnswerResult(state = "consumed", grant = StepUpGrant("key_device.remove", KEY_DEVICE, "device", "2026-11-03T12:05:00Z"))
        val failures = mutableMapOf<String, Exception>()
        /** Holds a call (by name) until completed. */
        val gates = mutableMapOf<String, CompletableDeferred<Unit>>()
        var onCall: (String) -> Unit = {}

        private suspend fun call(name: String, origin: String, access: String) {
            onCall(name)
            gates[name]?.await()
            calls += name
            servers += origin
            bearers += access
            failures[name]?.let { throw it }
        }

        override suspend fun me(origin: String, access: String): User { call("me", origin, access); return me }
        override suspend fun keyDevices(origin: String, access: String): List<KeyDevice> { call("keyDevices", origin, access); return devices }
        override suspend fun stepUp(origin: String, access: String, action: String, target: String): ApprovalChallenge {
            call("stepUp:$action:$target", origin, access)
            return stepUpChallenge
        }
        override suspend fun answer(origin: String, access: String, challengeId: String, decision: String, responseB64u: String, signatureB64u: String): AnswerResult {
            call("answer", origin, access)
            return answerResult
        }
        /** What the server answers a removal that went through. */
        var removed: (String) -> KeyDevice = { KeyDevice(id = it, label = "Pixel 8", state = KeyDeviceStatus.REVOKED) }
        override suspend fun removeKeyDevice(origin: String, access: String, id: String): KeyDevice {
            call("remove:$id", origin, access)
            return removed(id)
        }
    }

    private inner class Phone(scope: TestScope, uv: UvResult? = null, appInstallId: String = SigFixtures.DEVICE_ID) {
        val f = KeyDeviceFixture(scope.backgroundScope, uv).also { it.authState.setLoggedOut() } // a lapsed key phone
        val approval = FakeApprovalApi().apply { nextChallenge = SigFixtures.challengeFor(SigKind.DEVICE_LOGIN, 3, SigFixtures.V03_CONTEXT) }
        val api = FakeRemovalApi()
        val revoked = mutableListOf<Triple<String, String?, String?>>()
        val authApi: AuthApi = mockk {
            coEvery { logoutSession(any(), any(), any()) } answers { revoked += Triple(arg(0), arg(1), arg(2)); Unit }
        }
        // What every other part of the app sees: the stored session, and who is signed in.
        val prefs = InMemorySharedPreferences()
        val tokenStorage = TokenStorage(prefs).apply { saveServerUrl(SigFixtures.ORIGIN) }
        val sessions = mutableListOf<AuthSession>()
        /** Stands for the connection manager (and push, renewal): they start on a signed-in session. */
        var connects = 0
        val stored = prefs.all.toMap()
        val generation = tokenStorage.getSessionGeneration()
        /** The accounts whose kept push registration the removal asked to drop. */
        val pushDrops = mutableListOf<String>()
        val removal = PhoneRemoval(
            approval, api, authApi, f.signer, f.repo, f.answers, { appInstallId }, { pushDrops += it }, scope.backgroundScope,
        )

        init {
            scope.backgroundScope.launch(UnconfinedTestDispatcher(scope.testScheduler)) {
                f.authState.session.collect {
                    sessions += it
                    if (it is AuthSession.LoggedIn) connects++
                }
            }
        }

        /** Nothing of the removal reached storage, the app's sign-in state, or what starts on it. */
        fun assertAppUntouched(why: String = "") {
            assertEquals(why, stored, prefs.all.toMap())
            assertEquals(why, generation, tokenStorage.getSessionGeneration())
            assertNull(why, tokenStorage.getAccessToken())
            assertNull(why, tokenStorage.getRefreshToken())
            assertEquals(why, listOf<AuthSession>(AuthSession.LoggedOut), sessions.distinct())
            assertEquals(why, 0, connects)
            assertFalse(why, prefs.all.values.any { it == REMOVAL_ACCESS || it == REMOVAL_REFRESH })
        }
    }

    @Test fun `the removal runs to its end without the app ever holding its session`() = runTest {
        val p = Phone(this)
        assertEquals(RemovalResult.Removed, p.removal.run(activity))
        testScheduler.runCurrent()
        assertEquals(listOf("me", "stepUp:key_device.remove:$KEY_DEVICE", "answer", "remove:$KEY_DEVICE"), p.api.calls)
        // Every call named the key's own server and the removal's own bearer.
        assertEquals(setOf(SigFixtures.ORIGIN), p.api.servers)
        assertEquals(setOf(REMOVAL_ACCESS), p.api.bearers)
        assertEquals(SigFixtures.ORIGIN, p.approval.deviceLoginOrigin)
        assertEquals(SigFixtures.ORIGIN, p.approval.deviceLoginFinishOrigin)
        // Two prompts, both for the key the record names.
        assertEquals(2, p.f.verifier.prompts.size)
        assertEquals(listOf(p.f.verifier.keyIds.first()), p.f.verifier.keyIds.distinct())
        // The phone is no key any more; the server ended that session itself.
        assertNull(p.f.repo.record.value)
        assertFalse(p.f.keyStore.hasKey())
        assertTrue(p.revoked.isEmpty())
        p.assertAppUntouched()
        // What the phone kept of its push registration goes with the removal, for that account only.
        assertEquals(listOf(SigFixtures.USER_ID), p.pushDrops)
    }

    /**
     * The contract's answer to a removal is the key device itself, now revoked. A success that
     * names another key device, one that is not revoked, or nothing a key device could be read
     * from, is no answer: the phone stays a key and the removal's session is ended.
     */
    @Test fun `only the removal's own answer for this key device forgets it`() = runTest {
        for ((what, answer) in listOf<Pair<String, (String) -> KeyDevice>>(
            "another key device" to { _ -> KeyDevice(id = "12345678-1234-4234-8234-123456789012", label = "Pixel 8", state = KeyDeviceStatus.REVOKED) },
            "still active" to { id -> KeyDevice(id = id, label = "Pixel 8", state = KeyDeviceStatus.ACTIVE) },
            "a state nobody knows" to { id -> KeyDevice(id = id, label = "Pixel 8", state = KeyDeviceStatus.UNKNOWN) },
        )) {
            val p = Phone(this)
            p.api.removed = answer
            assertEquals(what, RemovalResult.Failed("UNEXPECTED_ANSWER"), p.removal.run(activity))
            testScheduler.runCurrent()
            assertNotNull(what, p.f.repo.record.value)
            assertTrue(what, p.f.keyStore.hasKey())
            assertTrue(what, p.f.store.writes.isEmpty())
            assertEquals(what, 1, p.revoked.size)
            assertTrue(what, p.pushDrops.isEmpty())
            p.assertAppUntouched(what)
        }
    }

    @Test fun `a removal that did not go through leaves the push registration alone`() = runTest {
        val refused = Phone(this)
        refused.api.failures["remove:$KEY_DEVICE"] = apiError(409, "LAST_STRONG_AUTHENTICATOR")
        refused.removal.run(activity)
        assertTrue(refused.pushDrops.isEmpty())
        val cancelled = Phone(this, uv = UvResult.Cancelled)
        cancelled.removal.run(activity)
        assertTrue(cancelled.pushDrops.isEmpty())
        val gone = Phone(this)
        gone.approval.failWith = apiError(404, "KEY_DEVICE_UNKNOWN")
        gone.removal.run(activity)
        assertTrue(gone.pushDrops.isEmpty())
    }

    @Test fun `a failing push clean-up does not undo a removal`() = runTest {
        val p = Phone(this)
        val removal = PhoneRemoval(
            p.approval, p.api, p.authApi, p.f.signer, p.f.repo, p.f.answers, { SigFixtures.DEVICE_ID },
            { throw IllegalStateException("push") }, backgroundScope,
        )
        assertEquals(RemovalResult.Removed, removal.run(activity))
        assertNull(p.f.repo.record.value)
    }

    /** The server lists the key device under the install id it was enrolled with: every request names that one. */
    @Test fun `the removal names the install id of the record, whatever id the app has now`() = runTest {
        val p = Phone(this, appInstallId = "0a0a0a0a-0a0a-4a0a-8a0a-0a0a0a0a0a0a")
        val rec = p.f.repo.record.value!!
        p.f.store.stored = rec.copy(installId = SigFixtures.DEVICE_ID)
        p.f.repo.reload()
        assertEquals(RemovalResult.Removed, p.removal.run(activity))
        assertTrue("deviceLoginChallenge:${SigFixtures.USER_ID}:${SigFixtures.DEVICE_ID}" in p.approval.calls)
    }

    @Test fun `it has no way to the stored session, the sign-in state, the connection or push`() {
        val takes = PhoneRemoval::class.java.constructors.flatMap { it.parameterTypes.toList() }.map { it.simpleName }
        for (shared in listOf("TokenStorage", "AuthState", "AuthRepository", "RealtimeConnectionManager", "PushRegistrar", "TokenRefresher", "KaikuHttpClient", "DeviceSignIn")) {
            assertFalse("PhoneRemoval takes $shared", shared in takes)
        }
    }

    private val points = listOf("challenge", "prompt-1", "sign-in", "me", "stepUp", "prompt-2", "answer", "remove")

    /** Makes the removal of [p] wait at [point] until the returned gate is completed. */
    private fun holdAt(p: Phone, point: String): CompletableDeferred<Unit> {
        val held = CompletableDeferred<Unit>()
        when (point) {
            "challenge" -> p.approval.challengeGate = held
            "prompt-1" -> p.f.verifier.gate = held
            "sign-in" -> p.approval.deviceLoginGate = held
            "prompt-2" -> p.api.onCall = { if (it.startsWith("stepUp")) p.f.verifier.gate = held }
            "me" -> p.api.gates["me"] = held
            "stepUp" -> p.api.gates["stepUp:key_device.remove:$KEY_DEVICE"] = held
            "answer" -> p.api.gates["answer"] = held
            else -> p.api.gates["remove:$KEY_DEVICE"] = held
        }
        return held
    }

    private val removalSession = Triple(SigFixtures.ORIGIN, REMOVAL_ACCESS as String?, REMOVAL_REFRESH as String?)

    /** Every point at which the removal waits: stopped there, nothing is on disk and nothing else changed. */
    @Test fun `stopped at any step it leaves nothing behind`() = runTest {
        for (point in points) {
            val p = Phone(this)
            holdAt(p, point)
            val job = launch { p.removal.run(activity) }
            testScheduler.runCurrent()
            assertTrue(point, job.isActive)
            // The app could die right here: storage is what the next start would find.
            p.assertAppUntouched("waiting at $point")
            assertNotNull(point, p.f.repo.record.value)

            job.cancel() // the screen went away
            testScheduler.runCurrent()

            p.assertAppUntouched("stopped at $point")
            assertNotNull(point, p.f.repo.record.value)
            assertTrue(point, p.f.keyStore.hasKey())
            assertTrue(point, p.f.store.writes.isEmpty())
            // A session it had obtained is ended on its server, with its own tokens, off the screen's scope.
            val hadSession = points.indexOf(point) > points.indexOf("sign-in")
            assertEquals(point, if (hadSession) listOf(removalSession) else emptyList(), p.revoked)
        }
    }

    /** At every such point: an ordinary sign-in made meanwhile is the app's session afterwards, untouched. */
    @Test fun `an ordinary sign-in made while the removal waits is not touched by the removal ending`() = runTest {
        for (point in points) for (end in listOf("stopped", "finished")) {
            val p = Phone(this)
            val held = holdAt(p, point)
            val job = launch { p.removal.run(activity) }
            testScheduler.runCurrent()
            // Meanwhile the person signs in the ordinary way (on this phone, with this account).
            p.tokenStorage.saveTokens("ordinary-access", "ordinary-refresh", 900, SigFixtures.USER_ID)
            p.f.authState.setLoggedIn(SigFixtures.USER_ID)
            val ordinary = p.f.authState.session.value
            val generation = p.tokenStorage.getSessionGeneration()

            if (end == "stopped") job.cancel() else held.complete(Unit)
            testScheduler.runCurrent()

            val why = "$point, $end"
            assertEquals(why, "ordinary-access", p.tokenStorage.getAccessToken())
            assertEquals(why, "ordinary-refresh", p.tokenStorage.getRefreshToken())
            assertEquals(why, generation, p.tokenStorage.getSessionGeneration())
            assertEquals(why, ordinary, p.f.authState.session.value)
            assertEquals(why, 1, p.connects)
            // The removal ended no session but its own.
            assertTrue(why, p.revoked.all { it == removalSession })
        }
    }

    @Test fun `a prompt the system cancelled (a rotation) ends the removal and its session`() = runTest {
        // First prompt: no session yet, nothing to end.
        val first = Phone(this, uv = UvResult.Cancelled)
        assertEquals(RemovalResult.Cancelled, first.removal.run(activity))
        testScheduler.runCurrent()
        assertTrue(first.revoked.isEmpty())
        first.assertAppUntouched()
        assertNotNull(first.f.repo.record.value)

        // Second prompt: the session exists and is ended.
        val p = Phone(this)
        val held = CompletableDeferred<Unit>()
        p.api.onCall = { if (it.startsWith("stepUp")) p.f.verifier.gate = held }
        val job = async { p.removal.run(activity) }
        testScheduler.runCurrent()
        // The prompt is dismissed with the old activity.
        job.cancel()
        testScheduler.runCurrent()
        assertEquals(1, p.revoked.size)
        p.assertAppUntouched()
        assertNotNull(p.f.repo.record.value)
    }

    @Test fun `the only key of the account is kept, and the session made for the removal is ended`() = runTest {
        val p = Phone(this)
        p.api.failures["remove:$KEY_DEVICE"] = apiError(409, "LAST_STRONG_AUTHENTICATOR")
        assertEquals(RemovalResult.Failed("LAST_STRONG_AUTHENTICATOR"), p.removal.run(activity))
        testScheduler.runCurrent()
        assertNotNull(p.f.repo.record.value)
        assertTrue(p.f.keyStore.hasKey())
        assertEquals(1, p.revoked.size)
        p.assertAppUntouched()
    }

    @Test fun `only the server's own answers for a removed key forget it`() = runTest {
        // At the removal itself (revoked between the confirmation and the request).
        val atDelete = Phone(this)
        atDelete.api.failures["remove:$KEY_DEVICE"] = apiError(403, "KEY_DEVICE_REVOKED")
        assertEquals(RemovalResult.KeyGone(invalidated = false), atDelete.removal.run(activity))
        assertNull(atDelete.f.repo.record.value)
        // At the sign-in the removal starts with.
        val atChallenge = Phone(this)
        atChallenge.approval.failWith = apiError(404, "KEY_DEVICE_UNKNOWN")
        assertEquals(RemovalResult.KeyGone(invalidated = false), atChallenge.removal.run(activity))
        assertNull(atChallenge.f.repo.record.value)
        val atFinish = Phone(this)
        atFinish.approval.deviceLoginFailure = apiError(403, "KEY_DEVICE_REVOKED")
        assertEquals(RemovalResult.KeyGone(invalidated = false), atFinish.removal.run(activity))
        assertNull(atFinish.f.repo.record.value)

        // The same words with another status, or from the confirmation step: the key stays.
        for ((where, x) in listOf(
            "remove:$KEY_DEVICE" to apiError(500, "KEY_DEVICE_REVOKED"),
            "remove:$KEY_DEVICE" to apiError(403, "KEY_DEVICE_UNKNOWN"),
            "stepUp:key_device.remove:$KEY_DEVICE" to apiError(404, "KEY_DEVICE_UNKNOWN"),
            "answer" to apiError(403, "KEY_DEVICE_REVOKED"),
            "me" to apiError(404, "KEY_DEVICE_UNKNOWN"),
        )) {
            val p = Phone(this)
            p.api.failures[where] = x
            assertEquals("$where ${x.status.value}", RemovalResult.Failed(x.code!!), p.removal.run(activity))
            assertNotNull("$where ${x.status.value}", p.f.repo.record.value)
            assertTrue(p.f.keyStore.hasKey())
        }
    }

    @Test fun `a removal that could not be reached or understood keeps the key`() = runTest {
        for (x in listOf<Exception>(IOException("offline"), kotlinx.serialization.SerializationException("not json"))) {
            val p = Phone(this)
            p.api.failures["remove:$KEY_DEVICE"] = x
            assertEquals(RemovalResult.Failed("NETWORK"), p.removal.run(activity))
            assertNotNull(p.f.repo.record.value)
            testScheduler.runCurrent()
            assertEquals(1, p.revoked.size)
            p.assertAppUntouched()
        }
    }

    @Test fun `a session for another account, or without full rights, removes nothing`() = runTest {
        val other = Phone(this)
        other.api.me = other.api.me.copy(id = "12345678-1234-4234-8234-123456789012")
        assertEquals(RemovalResult.Failed("OTHER_ACCOUNT"), other.removal.run(activity))
        assertEquals(listOf("me"), other.api.calls)
        val probation = Phone(this)
        probation.api.me = probation.api.me.copy(session = SessionInfo(SID, "probation", keyDeviceId = KEY_DEVICE))
        assertEquals(RemovalResult.Failed("FULL_SESSION_REQUIRED"), probation.removal.run(activity))
        testScheduler.runCurrent()
        assertEquals(1, probation.revoked.size)
    }

    @Test fun `the confirmation must be for this session and this phone`() = runTest {
        val p = Phone(this)
        p.api.me = p.api.me.copy(session = SessionInfo("12345678-1234-4234-8234-123456789012", "full", keyDeviceId = KEY_DEVICE))
        assertEquals(RemovalResult.Failed("CONTEXT_MISMATCH"), p.removal.run(activity))
        assertEquals(1, p.f.verifier.prompts.size) // never prompted for a confirmation that names another session
        assertFalse(p.api.calls.contains("answer"))
    }

    @Test fun `the key-device id comes from the server's list when the session does not name it`() = runTest {
        val p = Phone(this)
        p.api.me = p.api.me.copy(session = SessionInfo(SID, "full"))
        p.api.devices = listOf(KeyDevice(KEY_DEVICE, label = "Pixel 8", state = KeyDeviceStatus.ACTIVE, isCurrent = true))
        assertEquals(RemovalResult.Removed, p.removal.run(activity))
        assertTrue("remove:$KEY_DEVICE" in p.api.calls)
    }

    @Test fun `a removal answered for a record that is no longer the phone's forgets nothing`() = runTest {
        val p = Phone(this)
        val held = CompletableDeferred<Unit>().also { p.api.gates["remove:$KEY_DEVICE"] = it }
        val job = async { p.removal.run(activity) }
        testScheduler.runCurrent()
        // While the request is under way the phone is set up anew (another key, another record).
        p.f.repo.forget()
        val newKey = p.f.keyStore.generate(ByteArray(32), UvPolicy.choose(30, true, true) as UvDecision.Use).keyId
        p.f.repo.saveClaimed(LocalKeyDevice(null, SigFixtures.USER_ID, SigFixtures.ORIGIN, UvClass.PER_USE, "e2", "secret"))
        held.complete(Unit)
        assertEquals(RemovalResult.Failed("NOT_FORGOTTEN"), job.await())
        assertEquals(KeyDevicePhase.CLAIMED, p.f.repo.phase())
        assertEquals(newKey, p.f.keyStore.keyId())
    }

    @Test fun `a phone that is no active key starts no removal`() = runTest {
        val p = Phone(this)
        p.f.repo.forget()
        assertEquals(RemovalResult.KeyGone(invalidated = false), p.removal.run(activity))
        assertTrue(p.approval.calls.isEmpty())
        val claimed = Phone(this)
        claimed.f.repo.saveClaimed(LocalKeyDevice(null, SigFixtures.USER_ID, SigFixtures.ORIGIN, UvClass.PER_USE, "e2", "secret"))
        assertEquals(RemovalResult.KeyGone(invalidated = false), claimed.removal.run(activity))
        assertTrue(claimed.approval.calls.isEmpty())
    }

    @Test fun `a key Android invalidated is forgotten and said so`() = runTest {
        val p = Phone(this)
        p.f.keyStore.invalidated = true
        assertEquals(RemovalResult.KeyGone(invalidated = true), p.removal.run(activity))
        p.assertAppUntouched()
    }

    /**
     * A key that is a key device only by the server's word about this install: once the server
     * accepted this key's sign-in for the removal, the key is no longer provisional, so the
     * removal the server confirmed is also made on the phone and reported as done.
     */
    @Test fun `a provisional key whose removal the server confirmed is removed on the phone`() = runTest {
        val p = Phone(this)
        p.f.store.stored = p.f.store.stored!!.copy(provisionalFrom = io.wolftown.kaiku.data.keydevice.KeyDevicePhase.CLAIMED)
        p.f.repo.reload()
        assertEquals(RemovalResult.Removed, p.removal.run(activity))
        assertEquals(listOf("me", "stepUp:key_device.remove:$KEY_DEVICE", "answer", "remove:$KEY_DEVICE"), p.api.calls)
        assertNull(p.f.repo.record.value)
        assertFalse(p.f.keyStore.hasKey())
    }

    /** That note cannot be written: the server's removal stands, the phone keeps its record and says that saving failed. */
    @Test fun `a provisional key whose note cannot be written says that saving failed`() = runTest {
        val p = Phone(this)
        p.f.store.stored = p.f.store.stored!!.copy(provisionalFrom = io.wolftown.kaiku.data.keydevice.KeyDevicePhase.CLAIMED)
        p.f.repo.reload()
        p.f.store.failWrites = true
        assertEquals(RemovalResult.Failed("NOT_SAVED"), p.removal.run(activity))
        assertEquals(io.wolftown.kaiku.R.string.security_key_storage_not_saved, io.wolftown.kaiku.ui.auth.removalFailedText("NOT_SAVED"))
        assertTrue(p.f.keyStore.hasKey())
    }
}
