package io.wolftown.kaiku.approval

import androidx.fragment.app.FragmentActivity
import io.mockk.mockk
import io.wolftown.kaiku.testutil.SigFixtures.DEVICE_ID
import io.wolftown.kaiku.testutil.SigFixtures.EXPIRES_AT
import io.wolftown.kaiku.testutil.SigFixtures.ORIGIN
import io.wolftown.kaiku.testutil.SigFixtures.USER_ID
import io.wolftown.kaiku.testutil.SigFixtures.V01_CONTEXT
import io.wolftown.kaiku.testutil.SigFixtures.b64u
import io.wolftown.kaiku.testutil.SigFixtures.bytes
import io.wolftown.kaiku.testutil.SigFixtures.challengeFor
import io.wolftown.kaiku.testutil.SigFixtures.unb64u
import io.wolftown.kaiku.testutil.SigFixtures.verifyP256
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class DeviceSignerTest {

    private val keyStore = SoftwareApprovalKeyStore()
    private val activity: FragmentActivity = mockk(relaxed = true)
    private var pinned: String? = ORIGIN
    private val checks = ChallengeVerifier({ pinned }, { EXPIRES_AT - 30 })
    private val prompt = UvPrompt(title = 1, subtitle = "Sign in to Kaiku")

    @Test fun `signs exactly prefix decision lp response`() = runTest {
        keyStore.generate(ByteArray(32), UvPolicy.choose(30, true, true) as UvDecision.Use)
        val verifier = FakeUserVerifier(keyStore)
        val signer = DeviceSigner(keyStore, verifier, checks)
        val ch = challengeFor(SigKind.LOGIN, 1, V01_CONTEXT, "qr")

        val out = signer.sign(activity, ch, SigKind.LOGIN, USER_ID, DEVICE_ID, Decision.APPROVE, bytes(0xa0..0xbf), prompt)

        out as SignOutcome.Signed
        val expected = KaikuSig.message(unb64u(ch.signingPrefix), Decision.APPROVE, bytes(0xa0..0xbf))
        assertTrue(expected.contentEquals(verifier.signedMessages.single()))
        assertTrue(verifyP256(b64u(keyStore.publicKeySpki()!!), expected, out.signatureB64u))
        assertEquals(prompt, verifier.prompts.single())
        assertEquals(ch.challengeId, out.verified.challengeId)
    }

    @Test fun `rejected challenge never prompts`() = runTest {
        keyStore.generate(ByteArray(32), UvPolicy.choose(30, true, true) as UvDecision.Use)
        val verifier = FakeUserVerifier(keyStore)
        pinned = "https://other.example.com"
        val out = DeviceSigner(keyStore, verifier, checks)
            .sign(activity, challengeFor(SigKind.LOGIN, 1, V01_CONTEXT), SigKind.LOGIN, USER_ID, DEVICE_ID, Decision.APPROVE, ByteArray(0), prompt)
        assertEquals(SignOutcome.Rejected(RejectReason.ORIGIN), out)
        assertTrue(verifier.prompts.isEmpty())
    }

    @Test fun `invalidated key maps to KeyInvalidated`() = runTest {
        keyStore.generate(ByteArray(32), UvPolicy.choose(30, true, true) as UvDecision.Use)
        keyStore.invalidated = true
        val out = DeviceSigner(keyStore, FakeUserVerifier(keyStore), checks)
            .sign(activity, challengeFor(SigKind.LOGIN, 1, V01_CONTEXT), SigKind.LOGIN, USER_ID, DEVICE_ID, Decision.APPROVE, ByteArray(0), prompt)
        assertEquals(SignOutcome.Uv(UvResult.KeyInvalidated), out)
    }

    @Test fun `missing key maps to KeyInvalidated without a prompt`() = runTest {
        val verifier = FakeUserVerifier(keyStore)
        val out = DeviceSigner(keyStore, verifier, checks)
            .sign(activity, challengeFor(SigKind.LOGIN, 1, V01_CONTEXT), SigKind.LOGIN, USER_ID, DEVICE_ID, Decision.APPROVE, ByteArray(0), prompt)
        assertEquals(SignOutcome.Uv(UvResult.KeyInvalidated), out)
        assertTrue(verifier.prompts.isEmpty())
    }

    @Test fun `cancel maps to Cancelled and returns no signature`() = runTest {
        keyStore.generate(ByteArray(32), UvPolicy.choose(30, true, true) as UvDecision.Use)
        val verifier = FakeUserVerifier(keyStore, result = UvResult.Cancelled)
        val out = DeviceSigner(keyStore, verifier, checks)
            .sign(activity, challengeFor(SigKind.LOGIN, 1, V01_CONTEXT), SigKind.LOGIN, USER_ID, DEVICE_ID, Decision.REPORT, ByteArray(0), prompt)
        assertEquals(SignOutcome.Uv(UvResult.Cancelled), out)
    }

    @Test fun `an unexpected keystore failure is a failed result, not a crash`() = runTest {
        keyStore.generate(ByteArray(32), UvPolicy.choose(30, true, true) as UvDecision.Use)
        val throwing = object : UserVerifier {
            override suspend fun sign(activity: FragmentActivity, prompt: UvPrompt, message: ByteArray): UvResult =
                throw java.security.ProviderException("Keystore operation failed")
        }
        val out = DeviceSigner(keyStore, throwing, checks)
            .sign(activity, challengeFor(SigKind.LOGIN, 1, V01_CONTEXT), SigKind.LOGIN, USER_ID, DEVICE_ID, Decision.APPROVE, ByteArray(0), prompt)
        assertTrue(out is SignOutcome.Uv && out.result is UvResult.Failed)
    }

    @Test fun `an invalidated key runs the invalidation hook once`() = runTest {
        keyStore.generate(ByteArray(32), UvPolicy.choose(30, true, true) as UvDecision.Use)
        keyStore.invalidated = true
        var hooked = 0
        val out = DeviceSigner(keyStore, FakeUserVerifier(keyStore), checks, onKeyInvalidated = { hooked++ })
            .sign(activity, challengeFor(SigKind.LOGIN, 1, V01_CONTEXT), SigKind.LOGIN, USER_ID, DEVICE_ID, Decision.APPROVE, ByteArray(0), prompt)
        assertEquals(SignOutcome.Uv(UvResult.KeyInvalidated), out)
        assertEquals(1, hooked)
    }

    @Test fun `an invalidated key reported in another shape still counts as invalidated`() = runTest {
        keyStore.generate(ByteArray(32), UvPolicy.choose(30, true, true) as UvDecision.Use)
        var reported = 0
        val throwing = object : UserVerifier {
            override suspend fun sign(activity: FragmentActivity, prompt: UvPrompt, message: ByteArray): UvResult =
                throw java.security.UnrecoverableKeyException("User changed or deleted their auth credentials")
        }
        val out = DeviceSigner(keyStore, throwing, checks) { reported++ }
            .sign(activity, challengeFor(SigKind.LOGIN, 1, V01_CONTEXT, "qr"), SigKind.LOGIN, USER_ID, DEVICE_ID, Decision.APPROVE, bytes(0xa0..0xbf), prompt)
        assertEquals(SignOutcome.Uv(UvResult.KeyInvalidated), out)
        assertEquals(1, reported)
    }
}
