package io.wolftown.kaiku.data.auth

import io.mockk.coEvery
import io.mockk.mockk
import io.wolftown.kaiku.approval.SoftwareApprovalKeyStore
import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.approval.UvDecision
import io.wolftown.kaiku.approval.UvPolicy
import io.wolftown.kaiku.approval.keyId
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

    // ── One result of what an earlier version left, and everything waits for it ───────────────

    private fun keyStore() = SoftwareApprovalKeyStore().apply { generate(ByteArray(32), UvPolicy.choose(30, true, true) as UvDecision.Use) }

    /** The flag cannot be read, the record beside it can: the stored session is not restored as an ordinary one. */
    @Test fun `an unreadable flag beside a readable record keeps the session from being restored`() = runTest {
        store.legacyMark = true
        store.flagUnreadable = true
        tokenStorage.saveTokens("left-access", "left-refresh", 900, "u1")
        val repo = KeyDeviceRepository(store, keyStore(), AuthState(backgroundScope), backgroundScope)
        val gate = KeyStorageGate(repo, LegacyRemovalSession(store, tokenStorage, authApi, backgroundScope)) { "install-1" }
        assertFalse(gate.start())
        assertTrue(gate.unreadable())
        // Nothing was decided: the session is still in storage, and nothing may read it.
        assertEquals("left-access", tokenStorage.getAccessToken())
        // Readable again: the left-over session goes before anything restores it.
        store.flagUnreadable = false
        assertTrue(gate.retry())
        assertNull(tokenStorage.getAccessToken())
    }

    /** A readable flag beside a record that cannot be read: nothing is cleaned up on half a picture. */
    @Test fun `a readable flag beside an unreadable record decides nothing`() = runTest {
        store.legacyMark = true
        store.unreadable = true
        tokenStorage.saveTokens("left-access", "left-refresh", 900, "u1")
        val legacy = LegacyRemovalSession(store, tokenStorage, authApi, backgroundScope)
        assertFalse(legacy.endIfLeft())
        assertEquals("left-access", tokenStorage.getAccessToken())
        assertEquals(true, store.legacyMark)
    }

    /**
     * A clear of the flag that fails is tried again before anything is restored; when it fails
     * again the app stays closed (nothing reads the session, nobody signs in), so the flag can
     * never clear a later ordinary sign-in: at the next start it finds the same left-over again.
     */
    @Test fun `a flag that could not be cleared keeps everything waiting`() = runTest {
        store.legacyMark = true
        tokenStorage.saveTokens("left-access", "left-refresh", 900, "u1")
        val repo = KeyDeviceRepository(store, keyStore(), AuthState(backgroundScope), backgroundScope)
        // Fails once: tried again at once, and it is done.
        store.failFlagClears = 1
        val gate = KeyStorageGate(repo, LegacyRemovalSession(store, tokenStorage, authApi, backgroundScope)) { "install-1" }
        assertTrue(gate.start())
        assertEquals(false, store.legacyMark)
        // Fails twice: the gate stays closed.
        store.legacyMark = true
        tokenStorage.saveTokens("left-access-2", "left-refresh-2", 900, "u1")
        store.failFlagClears = 2
        val next = KeyStorageGate(repo, LegacyRemovalSession(store, tokenStorage, authApi, backgroundScope)) { "install-1" }
        assertFalse(next.start())
        assertTrue(next.unreadable())
        assertNull(tokenStorage.getAccessToken())
        assertEquals(true, store.legacyMark)
    }

    /** The gate is closed until what an earlier version left was looked at in this process. */
    @Test fun `the gate is closed until the start has settled what was left`() = runTest {
        val repo = KeyDeviceRepository(store, keyStore(), AuthState(backgroundScope), backgroundScope)
        val gate = KeyStorageGate(repo, LegacyRemovalSession(store, tokenStorage, authApi, backgroundScope)) { "install-1" }
        assertTrue(gate.unreadable())
        assertTrue(gate.start())
        assertFalse(gate.unreadable())
    }

    /**
     * An older key record without its install id, on an app that has kept none (it would have to
     * make a new one): the record notes that its id is lost, for good; it never takes an id made
     * later.
     */
    @Test fun `a record whose install id cannot be established is marked so, and never takes a new one`() = runTest {
        val repo = KeyDeviceRepository(store, keyStore(), AuthState(backgroundScope), backgroundScope)
        var kept: String? = null
        val ids = object : io.wolftown.kaiku.approval.DeviceIdProvider {
            override fun id(): String = "made-now".also { kept = it }
            override fun storedId(): String? = kept
        }
        val gate = KeyStorageGate(repo, LegacyRemovalSession(store, tokenStorage, authApi, backgroundScope), ids)
        gate.bindInstallId()
        assertTrue(store.stored!!.installIdLost)
        assertNull(store.stored!!.installId)
        // The app made an id meanwhile; the next start does not bind it.
        ids.id()
        gate.bindInstallId()
        assertNull(store.stored!!.installId)
        assertTrue(store.stored!!.installIdLost)
    }

    /**
     * An older key record without its install id, and no install id kept: a background process
     * (push registration) makes one before the first screen opens. The record never takes it, and
     * the server's honest "no key device" for that new id costs no key.
     */
    @Test fun `an install id made by background work before the first screen is never taken for the key's`() = runTest {
        store.stored = StoredKeyDevice(device, KeyDevicePhase.ACTIVE, keyId = null)
        val keys = keyStore()
        val repo = KeyDeviceRepository(store, keys, AuthState(backgroundScope), backgroundScope)
        var kept: String? = null
        val ids = object : io.wolftown.kaiku.approval.DeviceIdProvider {
            override fun id(): String = (kept ?: "made-by-push").also { kept = it }
            override fun storedId(): String? = kept
        }
        // A background process starts: the gate runs first, as in every process.
        KeyStorageGate(repo, LegacyRemovalSession(store, tokenStorage, authApi, backgroundScope), ids).start()
        // Then its push registration makes an id.
        ids.id()
        // Later the app is opened (a new process: the gate runs again).
        KeyStorageGate(repo, LegacyRemovalSession(store, tokenStorage, authApi, backgroundScope), ids).start()
        val rec = repo.record.value!!
        assertTrue(rec.installIdLost)
        assertNull(rec.installId)
        // "No key device" for whatever id is asked about keeps the key.
        val answers = io.wolftown.kaiku.data.keydevice.KeyAnswers(repo)
        val settled = answers.settle(rec, "https://chat.example.com", io.wolftown.kaiku.data.keydevice.Ask.DEVICE_LOGIN, io.wolftown.kaiku.data.keydevice.ServerAnswer(404, "KEY_DEVICE_UNKNOWN"))
        assertFalse(settled.forgotten)
        assertTrue(keys.hasKey())
    }
    /**
     * The first start cannot write that the record's install id is lost (a failing disk): the app
     * stays closed ("Try again"), nothing that needs the stored session (push registration) runs,
     * so no id is made that a later start could take for the key's.
     */
    @Test fun `a binding that did not reach storage keeps the app closed and costs no key`() = runTest {
        store.stored = StoredKeyDevice(device, KeyDevicePhase.ACTIVE, keyId = null)
        val keys = keyStore()
        val repo = KeyDeviceRepository(store, keys, AuthState(backgroundScope), backgroundScope)
        var kept: String? = null
        val ids = object : io.wolftown.kaiku.approval.DeviceIdProvider {
            override fun id(): String = (kept ?: "made-by-push").also { kept = it }
            override fun storedId(): String? = kept
        }
        store.failWrites = true
        val gate = KeyStorageGate(repo, LegacyRemovalSession(store, tokenStorage, authApi, backgroundScope), ids)
        assertFalse(gate.start())
        assertTrue(gate.unreadable())
        // Push registration reads the stored session only through the gate (as PushRegistrar does).
        if (!gate.unreadable()) ids.id()
        assertNull(kept)
        // The disk works again; the app is started anew.
        store.failWrites = false
        val next = KeyStorageGate(repo, LegacyRemovalSession(store, tokenStorage, authApi, backgroundScope), ids)
        assertTrue(next.start())
        val rec = repo.record.value!!
        assertTrue(rec.installIdLost)
        if (!next.unreadable()) ids.id()
        next.start()
        assertNull(repo.record.value!!.installId)
        val answers = io.wolftown.kaiku.data.keydevice.KeyAnswers(repo)
        val settled = answers.settle(repo.record.value!!, "https://chat.example.com", io.wolftown.kaiku.data.keydevice.Ask.DEVICE_LOGIN, io.wolftown.kaiku.data.keydevice.ServerAnswer(404, "KEY_DEVICE_UNKNOWN"))
        assertFalse(settled.forgotten)
        assertTrue(keys.hasKey())
    }

    // ── A write that did not reach the disk is not done (the store as Android keeps it) ────────

    /** The key store's file on preferences that behave like Android's after a failed commit. */
    private class Disk {
        val prefs = InMemorySharedPreferences()
        val store = io.wolftown.kaiku.data.keydevice.PrefsKeyDeviceStore(prefs)
    }

    /** The app's install id, kept in a file of its own that is not affected here. */
    private class Ids : io.wolftown.kaiku.approval.DeviceIdProvider {
        var kept: String? = null
        override fun id(): String = (kept ?: "made-by-push").also { kept = it }
        override fun storedId(): String? = kept
    }

    private fun kotlinx.coroutines.test.TestScope.gateOn(disk: Disk, keys: SoftwareApprovalKeyStore, ids: Ids): Pair<KeyStorageGate, KeyDeviceRepository> {
        val repo = KeyDeviceRepository(disk.store, keys, AuthState(backgroundScope), backgroundScope)
        return KeyStorageGate(repo, LegacyRemovalSession(disk.store, tokenStorage, authApi, backgroundScope), ids) to repo
    }

    /**
     * An older key record without its install id, the app has kept none, and the disk refuses the
     * note that the id is lost. Android still shows the note in memory; "Try again" opens nothing
     * while it cannot be written, so nothing can make an id meanwhile. After a process death the
     * old record is read again, the note is written then, and "no key device" costs no key.
     */
    @Test fun `try again does not take a binding that only reached memory`() = runTest {
        val disk = Disk()
        val keys = keyStore()
        assertTrue(disk.store.save(StoredKeyDevice(device, KeyDevicePhase.ACTIVE, keyId = keys.keyId())))
        val ids = Ids()
        disk.prefs.failingCommits = 100
        val (gate, _) = gateOn(disk, keys, ids)
        assertFalse(gate.start())
        // The disk still fails: "Try again" keeps the app closed, however often.
        assertFalse(gate.retry())
        assertFalse(gate.retry())
        assertTrue(gate.unreadable())
        // Push registration reads the stored session only through the gate (as PushRegistrar does).
        if (!gate.unreadable()) ids.id()
        assertNull(ids.kept)
        // Process death; the disk works again.
        disk.prefs.restart()
        disk.prefs.failingCommits = 0
        val (next, repo) = gateOn(disk, keys, ids)
        assertTrue(next.start())
        val rec = repo.record.value!!
        assertTrue(rec.installIdLost)
        assertNull(rec.installId)
        val answers = io.wolftown.kaiku.data.keydevice.KeyAnswers(repo)
        val settled = answers.settle(rec, "https://chat.example.com", io.wolftown.kaiku.data.keydevice.Ask.DEVICE_LOGIN, io.wolftown.kaiku.data.keydevice.ServerAnswer(404, "KEY_DEVICE_UNKNOWN"))
        assertFalse(settled.forgotten)
        assertTrue(keys.hasKey())
    }

    /** "Try again" opens the app once the pending note was written through, and only then; it is on the disk. */
    @Test fun `try again opens the app only after the binding was written through`() = runTest {
        val disk = Disk()
        val keys = keyStore()
        assertTrue(disk.store.save(StoredKeyDevice(device, KeyDevicePhase.ACTIVE, keyId = keys.keyId())))
        disk.prefs.failingCommits = 100
        val (gate, _) = gateOn(disk, keys, Ids())
        assertFalse(gate.start())
        assertFalse(gate.retry())
        disk.prefs.failingCommits = 0
        assertTrue(gate.retry())
        assertFalse(gate.unreadable())
        // What opened the app is on the disk: a process death reads the same.
        disk.prefs.restart()
        val rec = (disk.store.load() as io.wolftown.kaiku.data.keydevice.KeyStorage.Present).record
        assertTrue(rec.installIdLost)
    }

    /**
     * Both clears of an earlier version's flag fail: Android shows the flag as gone in memory.
     * "Try again" keeps the app closed while that cannot be written, so nobody signs in; after a
     * process death the flag is read again and clears only the same left-over session.
     */
    @Test fun `try again does not take a flag clear that only reached memory`() = runTest {
        val disk = Disk()
        val keys = keyStore()
        assertTrue(disk.store.save(StoredKeyDevice(device, KeyDevicePhase.ACTIVE, keyId = keys.keyId(), installId = "install-1")))
        assertTrue(disk.prefs.edit().putBoolean("removal_sign_in", true).commit())
        tokenStorage.saveTokens("left-access", "left-refresh", 900, "u1")
        disk.prefs.failingCommits = 100
        val (gate, _) = gateOn(disk, keys, Ids())
        assertFalse(gate.start())
        assertNull(tokenStorage.getAccessToken())
        // The disk still fails: closed.
        assertFalse(gate.retry())
        assertTrue(gate.unreadable())
        // Process death; the disk works again: the flag is back and is cleared, nothing else.
        disk.prefs.restart()
        disk.prefs.failingCommits = 0
        assertEquals(true, disk.store.legacyRemovalMark())
        val (next, _) = gateOn(disk, keys, Ids())
        assertTrue(next.start())
        assertEquals(false, disk.store.legacyRemovalMark())
        disk.prefs.restart()
        assertEquals(false, disk.store.legacyRemovalMark())
    }

    // ── What the closed gate says ────────────────────────────────────────────────────────────

    /**
     * A write of the start that did not reach the disk: the screen says that Kaiku could not save,
     * to free up space and try again, and never advises to clear the app's storage (that would
     * delete the key). Data that cannot be read keeps the present text and its last resort.
     */
    @Test fun `the closed gate tells a failed write from data that cannot be read`() = runTest {
        val disk = Disk()
        val keys = keyStore()
        assertTrue(disk.store.save(StoredKeyDevice(device, KeyDevicePhase.ACTIVE, keyId = keys.keyId())))
        disk.prefs.failingCommits = 100
        val (gate, _) = gateOn(disk, keys, Ids())
        assertFalse(gate.start())
        assertEquals(KeyStorageProblem.CANNOT_WRITE, gate.problem())
        assertFalse(gate.retry())
        assertEquals(KeyStorageProblem.CANNOT_WRITE, gate.problem())
        val write = keyStorageTexts(gate.problem())
        assertEquals(io.wolftown.kaiku.R.string.security_key_storage_not_saved, write.notice)
        assertNull(write.lastResort)
        assertEquals(io.wolftown.kaiku.R.string.common_try_again, write.button)
        // The disk takes writes again, but what it holds cannot be read now: the read text.
        disk.prefs.failingCommits = 0
        assertTrue(disk.prefs.edit().putString("phase", "NOT_A_PHASE").commit())
        assertFalse(gate.retry())
        assertEquals(KeyStorageProblem.CANNOT_READ, gate.problem())

        // A failed clear of an earlier version's flag is a failed write too.
        val flagDisk = Disk()
        assertTrue(flagDisk.store.save(StoredKeyDevice(device, KeyDevicePhase.ACTIVE, keyId = keys.keyId(), installId = "install-1")))
        assertTrue(flagDisk.prefs.edit().putBoolean("removal_sign_in", true).commit())
        flagDisk.prefs.failingCommits = 100
        val (flagGate, _) = gateOn(flagDisk, keys, Ids())
        assertFalse(flagGate.start())
        assertEquals(KeyStorageProblem.CANNOT_WRITE, flagGate.problem())

        // Key data that cannot be read: the present text, with its last resort.
        store.unreadable = true
        val repo = KeyDeviceRepository(store, keys, AuthState(backgroundScope), backgroundScope)
        val unreadable = KeyStorageGate(repo, LegacyRemovalSession(store, tokenStorage, authApi, backgroundScope)) { "install-1" }
        assertFalse(unreadable.start())
        assertEquals(KeyStorageProblem.CANNOT_READ, unreadable.problem())
        assertFalse(unreadable.retry())
        assertEquals(KeyStorageProblem.CANNOT_READ, unreadable.problem())
        val read = keyStorageTexts(unreadable.problem())
        assertEquals(io.wolftown.kaiku.R.string.security_key_storage_unreadable, read.notice)
        assertEquals(io.wolftown.kaiku.R.string.security_clear_storage_hint, read.lastResort)
    }
}
