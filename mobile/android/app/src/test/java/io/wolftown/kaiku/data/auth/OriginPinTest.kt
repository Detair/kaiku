package io.wolftown.kaiku.data.auth

import io.wolftown.kaiku.testutil.saveClaimed
import io.wolftown.kaiku.testutil.markActive
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

    @Test fun `a setup adopts its server only while the app is tied to none and holds no key record`() = runTest {
        val authState = AuthState(backgroundScope)
        val store = InMemoryKeyDeviceStore()
        val repo = KeyDeviceRepository(store, SoftwareApprovalKeyStore(), authState, backgroundScope)
        val saved = mutableListOf<String>()
        val tokens: TokenStorage = mockk(relaxed = true) {
            every { getServerUrl() } answers { serverUrl }
            every { saveServerUrl(any()) } answers { saved += firstArg<String>(); serverUrl = firstArg() }
        }
        val env = io.wolftown.kaiku.ui.auth.setup.AndroidSetupEnvironment(OriginPin(repo, authState, tokens), authState, tokens, io.wolftown.kaiku.ui.auth.setup.ServerAddressRule(repo, OriginPin(repo, authState, tokens)::pinned) { false })

        // Signed in on a server: tied to it.
        serverUrl = "https://other.example.org"
        authState.setLoggedIn(SigFixtures.USER_ID)
        assertEquals(false, env.adoptServer(SigFixtures.ORIGIN))
        authState.setLoggedOut()
        // A key record, whatever its phase.
        repo.saveClaimed(LocalKeyDevice(null, SigFixtures.USER_ID, "https://other.example.org", UvClass.PER_USE, "e1", "s"))
        assertEquals(false, env.adoptServer(SigFixtures.ORIGIN))
        // What it has saved cannot be read: nothing is known, nothing is adopted.
        store.unreadable = true
        repo.reload()
        assertEquals(false, env.adoptServer(SigFixtures.ORIGIN))
        assertEquals(emptyList<String>(), saved)

        store.unreadable = false
        store.stored = null
        repo.reload()
        assertEquals(true, env.adoptServer(SigFixtures.ORIGIN))
        assertEquals(listOf(SigFixtures.ORIGIN), saved)
    }

    /** A setup that adopted its server and is abandoned puts back exactly what was saved before, also "nothing". */
    @Test fun `an abandoned setup puts back the server that was saved before, or none`() = runTest {
        val authState = AuthState(backgroundScope)
        val repo = KeyDeviceRepository(InMemoryKeyDeviceStore(), SoftwareApprovalKeyStore(), authState, backgroundScope)
        val tokens = TokenStorage(io.wolftown.kaiku.testutil.InMemorySharedPreferences())
        val env = io.wolftown.kaiku.ui.auth.setup.AndroidSetupEnvironment(OriginPin(repo, authState, tokens), authState, tokens, io.wolftown.kaiku.ui.auth.setup.ServerAddressRule(repo, OriginPin(repo, authState, tokens)::pinned) { false })

        // No server was saved (a build without a default one).
        val before = env.savedServer()
        assertNull(before)
        assertEquals(true, env.adoptServer(SigFixtures.ORIGIN))
        assertEquals(SigFixtures.ORIGIN, tokens.getServerUrl())
        env.restoreServer(before)
        assertNull(tokens.getServerUrl())

        // A server the person had typed in.
        tokens.saveServerUrl("https://typed.example.net")
        val typed = env.savedServer()
        assertEquals(true, env.adoptServer(SigFixtures.ORIGIN))
        env.restoreServer(typed)
        assertEquals("https://typed.example.net", tokens.getServerUrl())
    }

    @Test fun `clearing the saved server starts a new session generation and leaves the rest`() {
        val tokens = TokenStorage(io.wolftown.kaiku.testutil.InMemorySharedPreferences())
        tokens.saveServerUrl("https://chat.example.com")
        tokens.saveTokens("a", "r", 900, "u1")
        val generation = tokens.getSessionGeneration()
        tokens.clearServerUrl()
        assertNull(tokens.getServerUrl())
        assertEquals("a", tokens.getAccessToken())
        assertEquals(true, tokens.getSessionGeneration() != generation)
    }
}
