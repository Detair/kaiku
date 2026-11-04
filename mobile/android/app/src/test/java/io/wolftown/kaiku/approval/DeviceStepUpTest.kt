package io.wolftown.kaiku.approval

import androidx.fragment.app.FragmentActivity
import io.mockk.coEvery
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.data.auth.DeviceStepUpProvider
import io.wolftown.kaiku.data.auth.PasskeyFailure
import io.wolftown.kaiku.data.auth.StepUpProvider
import io.wolftown.kaiku.data.auth.StepUpFailure
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.LocalKeyDevice
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.domain.model.AnswerResult
import io.wolftown.kaiku.domain.model.SessionInfo
import io.wolftown.kaiku.domain.model.StepUpAction
import io.wolftown.kaiku.domain.model.StepUpGrant
import io.wolftown.kaiku.domain.model.User
import io.wolftown.kaiku.testutil.FakeApprovalApi
import io.wolftown.kaiku.testutil.InMemoryKeyDeviceStore
import io.wolftown.kaiku.testutil.SigFixtures
import io.wolftown.kaiku.testutil.apiError
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class DeviceStepUpTest {

    private val activity: FragmentActivity = mockk(relaxed = true)
    private val keyStore = SoftwareApprovalKeyStore().apply {
        generate(ByteArray(32), UvPolicy.choose(30, true, true) as UvDecision.Use)
    }
    private val api = FakeApprovalApi().apply {
        nextChallenge = SigFixtures.challengeFor(SigKind.DEVICE_STEPUP, 4, SigFixtures.V04_CONTEXT)
    }
    private val grant = StepUpGrant("key_device.remove", "99999999-9999-4999-8999-999999999999", "device", "2026-11-03T12:05:00Z")
    private val authApi: AuthApi = mockk()
    private val checks = ChallengeVerifier({ SigFixtures.ORIGIN }, { SigFixtures.EXPIRES_AT - 30 })
    private lateinit var verifier: FakeUserVerifier
    private lateinit var repo: KeyDeviceRepository

    private fun TestScope.stepUp(sid: String = "77777777-7777-4777-8777-777777777777", keyDevice: Boolean = true): DeviceStepUp {
        val authState = AuthState(backgroundScope)
        repo = KeyDeviceRepository(InMemoryKeyDeviceStore(), keyStore, authState, backgroundScope)
        authState.setLoggedIn(SigFixtures.USER_ID)
        if (keyDevice) {
            repo.saveClaimed(LocalKeyDevice("kd1", SigFixtures.USER_ID, SigFixtures.ORIGIN, UvClass.PER_USE, "e1", null))
            repo.markActive(SigFixtures.USER_ID, "kd1")
        }
        coEvery { authApi.getMe() } returns User(SigFixtures.USER_ID, "alice", "Alice", session = SessionInfo(sid, "full"))
        api.answerResult = AnswerResult(state = "consumed", grant = grant)
        verifier = FakeUserVerifier(keyStore)
        return DeviceStepUp(api, DeviceSigner(keyStore, verifier, checks), repo, { SigFixtures.DEVICE_ID }, authApi)
    }

    @Test fun `step up signs the device_stepup challenge and returns the grant`() = runTest {
        val out = stepUp().stepUp(activity, StepUpAction.KEY_DEVICE_REMOVE, "99999999-9999-4999-8999-999999999999", "Pixel 8")
        assertEquals(StepUpOutcome.Granted(grant), out)
        assertEquals("stepUp:key_device.remove:99999999-9999-4999-8999-999999999999", api.calls.first())
        val (decision, response, signature) = api.lastAnswer!!
        assertEquals("approve", decision)
        assertEquals("", response)
        val m = KaikuSig.message(SigFixtures.unb64u(api.nextChallenge!!.signingPrefix), Decision.APPROVE, ByteArray(0))
        assertTrue(SigFixtures.verifyP256(SigFixtures.b64u(keyStore.publicKeySpki()!!), m, signature))
        assertEquals(1, verifier.prompts.size)
    }

    @Test fun `step up refuses a context with another sid`() = runTest {
        val out = stepUp(sid = "12345678-1234-4234-8234-123456789012")
            .stepUp(activity, StepUpAction.KEY_DEVICE_REMOVE, "99999999-9999-4999-8999-999999999999", "Pixel 8")
        assertEquals(StepUpOutcome.Failed("CONTEXT_MISMATCH"), out)
        assertTrue(verifier.prompts.isEmpty())
        assertTrue(api.calls.none { it.startsWith("answer") })
    }

    @Test fun `step up refuses a context for another action or target`() = runTest {
        val s = stepUp()
        assertEquals(StepUpOutcome.Failed("CONTEXT_MISMATCH"), s.stepUp(activity, StepUpAction.PASSKEY_REMOVE, "99999999-9999-4999-8999-999999999999", ""))
        assertEquals(StepUpOutcome.Failed("CONTEXT_MISMATCH"), s.stepUp(activity, StepUpAction.KEY_DEVICE_REMOVE, "others", ""))
        assertTrue(verifier.prompts.isEmpty())
    }

    @Test fun `not a key device returns NotKeyDevice`() = runTest {
        assertEquals(StepUpOutcome.NotKeyDevice, stepUp(keyDevice = false).stepUp(activity, StepUpAction.ENROLLMENT_ISSUE, "", "Add a phone"))
        val s = stepUp()
        api.failWith = apiError(403, "KEY_DEVICE_REQUIRED")
        assertEquals(StepUpOutcome.NotKeyDevice, s.stepUp(activity, StepUpAction.ENROLLMENT_ISSUE, "", "Add a phone"))
        api.failWith = apiError(403, "FULL_SESSION_REQUIRED")
        assertEquals(StepUpOutcome.ProbationSession, s.stepUp(activity, StepUpAction.ENROLLMENT_ISSUE, "", "Add a phone"))
    }

    @Test fun `cancelled prompt gives no grant`() = runTest {
        stepUp()
        val cancelling = DeviceStepUp(api, DeviceSigner(keyStore, FakeUserVerifier(keyStore, UvResult.Cancelled), checks),
            repo, { SigFixtures.DEVICE_ID }, authApi)
        assertEquals(StepUpOutcome.Cancelled, cancelling.stepUp(activity, StepUpAction.KEY_DEVICE_REMOVE, "99999999-9999-4999-8999-999999999999", ""))
        assertTrue(api.calls.none { it.startsWith("answer") })
    }

    @Test fun `device provider maps the grant into LP's StepUpProvider result`() = runTest {
        val provider = DeviceStepUpProvider(stepUp())
        assertEquals(Result.success(grant), provider.stepUp(activity, "key_device.remove", "99999999-9999-4999-8999-999999999999"))
        val failed = provider.stepUp(activity, "no.such.action", "")
        assertTrue(failed.exceptionOrNull() is StepUpFailure)
        api.failWith = apiError(500, "INTERNAL_ERROR")
        assertEquals(StepUpOutcome.Failed("INTERNAL_ERROR"), (provider.stepUp(activity, "passkey.add", "").exceptionOrNull() as StepUpFailure).outcome)
    }

    /** LP's PasskeysViewModel reads LP's failure types: a cancel shows nothing, probation says how to get a full session. */
    @Test fun `device provider speaks LP's failure types for cancel and probation`() = runTest {
        stepUp()
        val cancelling = DeviceStepUpProvider(
            DeviceStepUp(api, DeviceSigner(keyStore, FakeUserVerifier(keyStore, UvResult.Cancelled), checks), repo, { SigFixtures.DEVICE_ID }, authApi),
        )
        assertEquals(PasskeyFailure.Cancelled, cancelling.stepUp(activity, "key_device.remove", "99999999-9999-4999-8999-999999999999").exceptionOrNull())
        api.failWith = apiError(403, "FULL_SESSION_REQUIRED")
        val probation = DeviceStepUpProvider(stepUp()).stepUp(activity, "passkey.add", "").exceptionOrNull()
        assertEquals("FULL_SESSION_REQUIRED", (probation as PasskeyFailure.Server).code)
    }

    /** DC1: the key's step-up only on the key phone; elsewhere LP's passkey step-up gives `passkey.add`. */
    @Test fun `device provider falls back to the passkey step-up when this phone is not the key`() = runTest {
        val passkeyGrant = StepUpGrant("passkey.add", "", "passkey", "2026-11-03T12:05:00Z")
        val asked = mutableListOf<String>()
        val passkeys = object : StepUpProvider {
            override suspend fun stepUp(activity: android.app.Activity, action: String, target: String): Result<StepUpGrant> {
                asked += "$action:$target"
                return Result.success(passkeyGrant)
            }
        }
        assertEquals(Result.success(passkeyGrant), DeviceStepUpProvider(stepUp(keyDevice = false), passkeys).stepUp(activity, "passkey.add", ""))
        assertEquals(listOf("passkey.add:"), asked)
        assertTrue(verifier.prompts.isEmpty())

        // The server says the session is not the key's own (KEY_DEVICE_REQUIRED): the same fallback.
        val s = stepUp()
        api.failWith = apiError(403, "KEY_DEVICE_REQUIRED")
        assertEquals(Result.success(passkeyGrant), DeviceStepUpProvider(s, passkeys).stepUp(activity, "passkey.add", ""))
        assertEquals(2, asked.size)

        // Without a fallback the provider still says why.
        val none = DeviceStepUpProvider(stepUp(keyDevice = false)).stepUp(activity, "passkey.add", "").exceptionOrNull()
        assertEquals(StepUpOutcome.NotKeyDevice, (none as StepUpFailure).outcome)
    }

    @Test fun `the module binds the key phone's provider for LP's qualifier with the passkey fallback`() = runTest {
        val passkeyApi: io.wolftown.kaiku.data.api.PasskeyApi = mockk()
        coEvery { passkeyApi.stepUpStart(any(), any()) } throws java.io.IOException("offline")
        val passkeys = io.wolftown.kaiku.data.auth.PasskeyStepUp(passkeyApi, mockk())
        val bound = io.wolftown.kaiku.di.ApprovalModule.provideDeviceStepUpProvider(stepUp(keyDevice = false), passkeys)
        assertTrue(bound is DeviceStepUpProvider)
        assertTrue(bound.stepUp(activity, "passkey.add", "").exceptionOrNull() is java.io.IOException)
        io.mockk.coVerify(exactly = 1) { passkeyApi.stepUpStart("passkey.add", "") }
    }

    @Test fun `every action has a wire name from the common rules table`() {
        assertEquals(22, StepUpAction.entries.size)
        assertEquals(StepUpAction.ADMIN_RECOVERY_CONTACT_SET, StepUpAction.fromWire("admin.recovery_contact.set"))
        assertEquals(null, StepUpAction.fromWire("admin.unknown"))
    }

    @Test fun `an invalidated key during a step-up reports and forgets the device`() = runTest {
        stepUp()
        keyStore.invalidated = true
        val authRepository: io.wolftown.kaiku.data.repository.AuthRepository = mockk { every { hasLiveSession() } returns true }
        val invalidation = io.wolftown.kaiku.data.keydevice.KeyInvalidation(api, repo, authRepository)
        val s = DeviceStepUp(api, DeviceSigner(keyStore, FakeUserVerifier(keyStore), checks, invalidation::handle), repo, { SigFixtures.DEVICE_ID }, authApi)
        assertEquals(StepUpOutcome.Failed("KEY_INVALIDATED"), s.stepUp(activity, StepUpAction.KEY_DEVICE_REMOVE, "99999999-9999-4999-8999-999999999999", ""))
        assertTrue("reportInvalidated:kd1" in api.calls)
        assertEquals(io.wolftown.kaiku.data.keydevice.KeyDeviceState.None, repo.current())
    }

    @Test fun `the prompt shows only the signed target label`() = runTest {
        val s = stepUp()
        api.nextChallenge = SigFixtures.challengeFor(SigKind.DEVICE_STEPUP, 4, SigFixtures.V04_CONTEXT.replace("\"target_label\":\"Pixel 8\"", "\"target_label\":\"\""))
        every { activity.getString(any(), any()) } answers { "label=" + (secondArg<Array<Any>>().firstOrNull() ?: "") }
        s.stepUp(activity, StepUpAction.KEY_DEVICE_REMOVE, "99999999-9999-4999-8999-999999999999", "Evil label")
        assertEquals("label=", verifier.prompts.single().subtitle)
    }
}
