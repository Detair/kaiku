package io.wolftown.kaiku.data.auth

import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.approval.ChallengeRejected
import io.wolftown.kaiku.approval.ChallengeVerifier
import io.wolftown.kaiku.approval.RejectReason
import io.wolftown.kaiku.approval.SigKind
import io.wolftown.kaiku.approval.SoftwareApprovalKeyStore
import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.LocalKeyDevice
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.testutil.InMemoryKeyDeviceStore
import io.wolftown.kaiku.testutil.SigFixtures
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.fail
import org.junit.Test

/**
 * B.1 / coordinator call 6: a key ties every device signature to the key's
 * server (ApprovalModule wires [OriginPin.signingOrigin] into the verifier).
 */
class OriginPinTest {

    private var serverUrl: String? = null
    private val tokenStorage: TokenStorage = mockk(relaxed = true) { every { getServerUrl() } answers { serverUrl } }

    private fun kotlinx.coroutines.test.TestScope.pin(withKey: Boolean): OriginPin {
        val authState = AuthState(backgroundScope)
        val repo = KeyDeviceRepository(InMemoryKeyDeviceStore(), SoftwareApprovalKeyStore(), authState, backgroundScope)
        if (withKey) {
            repo.saveClaimed(LocalKeyDevice("kd1", SigFixtures.USER_ID, SigFixtures.ORIGIN, UvClass.PER_USE, "e1", null))
            repo.markActive(SigFixtures.USER_ID, "kd1")
        }
        return OriginPin(repo, authState, tokenStorage)
    }

    @Test fun `signatures are pinned to the key's server, not a changed saved server`() = runTest {
        serverUrl = "https://other.example.org"
        val p = pin(withKey = true)
        assertEquals(SigFixtures.ORIGIN, p.signingOrigin())

        val verifier = ChallengeVerifier(p::signingOrigin) { SigFixtures.EXPIRES_AT - 30 }
        // A challenge of the key's server passes the origin check even though the saved server differs.
        verifier.verify(SigFixtures.challengeFor(SigKind.DEVICE_LOGIN, 3, SigFixtures.V03_CONTEXT), SigKind.DEVICE_LOGIN,
            SigFixtures.USER_ID, SigFixtures.DEVICE_ID)
    }

    @Test fun `a lost saved server still pins to the key's server`() = runTest {
        serverUrl = null
        assertEquals(SigFixtures.ORIGIN, pin(withKey = true).signingOrigin())
    }

    @Test fun `without a key the configured server is the signing origin`() = runTest {
        serverUrl = "https://Chat.Example.com:443"
        assertEquals(SigFixtures.ORIGIN, pin(withKey = false).signingOrigin())
        serverUrl = null
        assertNull(pin(withKey = false).signingOrigin())
    }

    @Test fun `a challenge of another server is refused for a key device`() = runTest {
        serverUrl = "https://other.example.org"
        val verifier = ChallengeVerifier(pin(withKey = true)::signingOrigin) { SigFixtures.EXPIRES_AT - 30 }
        val other = SigFixtures.challengeFor(
            SigKind.DEVICE_LOGIN, 3, SigFixtures.V03_CONTEXT,
            prefix = SigFixtures.prefixFor(
                SigKind.DEVICE_LOGIN, SigFixtures.challengeId(3),
                io.wolftown.kaiku.approval.SigContext.fromJson(SigKind.DEVICE_LOGIN, SigFixtures.json(SigFixtures.V03_CONTEXT)),
                origin = "https://other.example.org",
            ),
        )
        try {
            verifier.verify(other, SigKind.DEVICE_LOGIN, SigFixtures.USER_ID, SigFixtures.DEVICE_ID)
            fail("expected ORIGIN")
        } catch (e: ChallengeRejected) {
            assertEquals(RejectReason.ORIGIN, e.reason)
        }
    }
}
