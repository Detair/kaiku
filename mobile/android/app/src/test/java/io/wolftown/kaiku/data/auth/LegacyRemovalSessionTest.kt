package io.wolftown.kaiku.data.auth

import io.mockk.coEvery
import io.mockk.mockk
import io.wolftown.kaiku.approval.SoftwareApprovalKeyStore
import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.approval.UvDecision
import io.wolftown.kaiku.approval.UvPolicy
import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.data.keydevice.KeyDevicePhase
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.KeyDeviceState
import io.wolftown.kaiku.data.keydevice.LocalKeyDevice
import io.wolftown.kaiku.data.keydevice.StoredKeyDevice
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.testutil.InMemoryKeyDeviceStore
import io.wolftown.kaiku.testutil.InMemorySharedPreferences
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** A phone updated from a version that stored the session of a removal that stopped half way. */
class LegacyRemovalSessionTest {

    private val device = LocalKeyDevice("kd1", "u1", "https://chat.example.com", UvClass.PER_USE, "e1", null, "Oma")
    private val store = InMemoryKeyDeviceStore().apply { stored = StoredKeyDevice(device, KeyDevicePhase.ACTIVE) }
    private val tokenStorage = TokenStorage(InMemorySharedPreferences()).apply { saveServerUrl("https://chat.example.com") }
    private val ended = mutableListOf<Triple<String, String?, String?>>()
    private val authApi: AuthApi = mockk { coEvery { logoutSession(any(), any(), any()) } answers { ended += Triple(arg(0), arg(1), arg(2)); Unit } }

    @Test fun `a left-over removal session is taken out of storage before anything reads it`() = runTest {
        store.legacyMark = true
        tokenStorage.saveTokens("left-access", "left-refresh", 900, "u1")
        val keyStore = SoftwareApprovalKeyStore().apply { generate(ByteArray(32), UvPolicy.choose(30, true, true) as UvDecision.Use) }

        LegacyRemovalSession(store, tokenStorage, authApi, backgroundScope).endIfLeft()

        // Synchronously: no stored session, no flag.
        assertNull(tokenStorage.getAccessToken())
        assertNull(tokenStorage.getRefreshToken())
        assertEquals(false, store.legacyRemovalMark())
        // The app then starts as a key phone that is signed out: "Confirm it's you".
        val authState = AuthState(backgroundScope).apply { initialize(tokenStorage) }
        val repo = KeyDeviceRepository(store, keyStore, authState, backgroundScope)
        assertEquals(KeyDeviceState.Lapsed("u1"), repo.current())
        assertTrue(keyStore.hasKey())
        assertEquals("https://chat.example.com", tokenStorage.getServerUrl())
        testScheduler.runCurrent()
        assertEquals(listOf(Triple("https://chat.example.com", "left-access" as String?, "left-refresh" as String?)), ended)
    }

    @Test fun `without the flag a stored session is the person's own and stays`() = runTest {
        tokenStorage.saveTokens("own-access", "own-refresh", 900, "u1")
        LegacyRemovalSession(store, tokenStorage, authApi, backgroundScope).endIfLeft()
        testScheduler.runCurrent()
        assertEquals("own-access", tokenStorage.getAccessToken())
        assertTrue(ended.isEmpty())
    }

    @Test fun `while the store cannot be read nothing is decided, and the gate runs it once it can`() = runTest {
        store.legacyMark = true
        store.unreadable = true
        tokenStorage.saveTokens("left-access", "left-refresh", 900, "u1")
        val legacy = LegacyRemovalSession(store, tokenStorage, authApi, backgroundScope)
        legacy.endIfLeft()
        assertEquals("left-access", tokenStorage.getAccessToken())

        val repo = KeyDeviceRepository(store, SoftwareApprovalKeyStore(), AuthState(backgroundScope), backgroundScope)
        val gate = KeyStorageGate(repo, legacy) { "install-1" }
        assertTrue(gate.unreadable())
        assertFalse(gate.retry())
        assertEquals("left-access", tokenStorage.getAccessToken())

        store.unreadable = false
        assertTrue(gate.retry())
        assertFalse(gate.unreadable())
        assertNull(tokenStorage.getAccessToken())
        assertEquals(false, store.legacyRemovalMark())
    }

    @Test fun `the flag stays until the session is really out of storage`() = runTest {
        store.legacyMark = true
        val failing = FailingCommitPrefs()
        val tokens = TokenStorage(failing).apply { saveServerUrl("https://chat.example.com") }
        tokens.saveTokens("left-access", "left-refresh", 900, "u1")
        failing.failCommits = true
        LegacyRemovalSession(store, tokens, authApi, backgroundScope).endIfLeft()
        assertEquals(true, store.legacyRemovalMark())
        testScheduler.runCurrent()
        assertTrue(ended.isEmpty())
        // The next start tries again.
        failing.failCommits = false
        LegacyRemovalSession(store, tokens, authApi, backgroundScope).endIfLeft()
        assertNull(tokens.getAccessToken())
        assertEquals(false, store.legacyRemovalMark())
    }

    /** Preferences whose commits can be made to fail (a full disk). */
    private class FailingCommitPrefs(private val inner: InMemorySharedPreferences = InMemorySharedPreferences()) :
        android.content.SharedPreferences by inner {
        var failCommits = false
        override fun edit(): android.content.SharedPreferences.Editor {
            val e = inner.edit()
            return object : android.content.SharedPreferences.Editor by e {
                override fun commit(): Boolean = if (failCommits) false else e.commit()
                override fun putString(key: String?, value: String?) = apply { e.putString(key, value) }
                override fun remove(key: String?) = apply { e.remove(key) }
                override fun putLong(key: String?, value: Long) = apply { e.putLong(key, value) }
            }
        }
    }

    /**
     * A key record from before records named their install id takes the id the app has at the
     * next start. From then on a question about that key device names this id, whatever becomes
     * of the app's own.
     */
    @Test fun `at a start a key record without an install id takes the app's, and keeps it`() = runTest {
        val keyStore = SoftwareApprovalKeyStore().apply {
            generate(ByteArray(32), io.wolftown.kaiku.approval.UvPolicy.choose(30, true, true) as io.wolftown.kaiku.approval.UvDecision.Use)
        }
        store.stored = io.wolftown.kaiku.data.keydevice.StoredKeyDevice(
            io.wolftown.kaiku.data.keydevice.LocalKeyDevice("kd1", "u1", "https://chat.example.com", io.wolftown.kaiku.approval.UvClass.PER_USE, "e1", null),
            io.wolftown.kaiku.data.keydevice.KeyDevicePhase.ACTIVE,
        )
        val repo = KeyDeviceRepository(store, keyStore, AuthState(backgroundScope), backgroundScope)
        var appId = "install-1"
        var asked = 0
        val gate = KeyStorageGate(repo, LegacyRemovalSession(store, tokenStorage, authApi, backgroundScope)) { asked++; appId }
        gate.bindInstallId()
        assertEquals("install-1", store.stored!!.installId)
        // The app's own id changes later (its file was lost and made anew): the record keeps its own.
        appId = "install-2"
        gate.bindInstallId()
        assertEquals("install-1", store.stored!!.installId)
        assertEquals(1, asked)
    }

    @Test fun `a phone without a key record is not given an install id for it`() = runTest {
        store.stored = null
        val repo = KeyDeviceRepository(store, SoftwareApprovalKeyStore(), AuthState(backgroundScope), backgroundScope)
        var asked = 0
        KeyStorageGate(repo, LegacyRemovalSession(store, tokenStorage, authApi, backgroundScope)) { asked++; "install-1" }.bindInstallId()
        assertEquals(0, asked)
        assertTrue(store.writes.isEmpty())
    }
}
