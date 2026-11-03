package io.wolftown.kaiku.data.local

import android.content.SharedPreferences
import io.wolftown.kaiku.testutil.InMemorySharedPreferences
import io.wolftown.kaiku.testutil.testValueCipher
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.security.GeneralSecurityException

class LegacyTokenMigrationTest {

    private val target = KeystoreWrappedPreferences(InMemorySharedPreferences(), testValueCipher())

    @Test fun `migration copies tokens and deletes the legacy store`() {
        val legacy = InMemorySharedPreferences()
        legacy.edit()
            .putString("access_token", "a").putString("refresh_token", "r").putLong("expires_at", 99L)
            .putString("user_id", "u").putString("server_url", "https://chat.example.com")
            .putString("unrelated", "x")
            .commit()
        var deleted = false

        val ok = LegacyTokenMigration.run(
            legacyExists = true,
            openLegacy = { legacy },
            deleteLegacy = { deleted = true },
            target = target,
            keys = TokenStorage.STORED_KEYS,
        )

        assertTrue(ok)
        assertTrue(deleted)
        val storage = TokenStorage(target)
        assertEquals("a", storage.getAccessToken())
        assertEquals("r", storage.getRefreshToken())
        assertEquals(99L, storage.getAccessTokenExpiresAtMillis())
        assertEquals("u", storage.getUserId())
        assertEquals("https://chat.example.com", storage.getServerUrl())
        assertNull(target.getString("unrelated", null))
    }

    @Test fun `no legacy store does nothing`() {
        var opened = false
        assertTrue(LegacyTokenMigration.run(false, { opened = true; null }, {}, target, TokenStorage.STORED_KEYS))
        assertFalse(opened)
    }

    @Test fun `damaged legacy store is dropped without crashing`() {
        var deleted = false
        // Cannot be opened at all (key lost after a restore).
        assertFalse(LegacyTokenMigration.run(true, { throw GeneralSecurityException("x") }, { deleted = true }, target, TokenStorage.STORED_KEYS))
        assertTrue(deleted)
        // Opens, but reading fails.
        deleted = false
        val unreadable = object : SharedPreferences by InMemorySharedPreferences() {
            override fun getAll(): MutableMap<String, *> = throw SecurityException("x")
        }
        assertFalse(LegacyTokenMigration.migrate(unreadable, target, TokenStorage.STORED_KEYS))
        assertFalse(LegacyTokenMigration.run(true, { unreadable }, { deleted = true }, target, TokenStorage.STORED_KEYS))
        assertTrue(deleted)
        assertNull(TokenStorage(target).getAccessToken())
    }

    @Test fun `a failing write does not stop the app from starting`() {
        val legacy = InMemorySharedPreferences().apply { edit().putString("access_token", "a").putString("refresh_token", "r").commit() }
        val broken = object : ValueCipher {
            override fun encrypt(plain: ByteArray, aad: ByteArray): ByteArray = throw java.security.ProviderException("keystore")
            override fun decrypt(blob: ByteArray, aad: ByteArray): ByteArray = throw java.security.ProviderException("keystore")
        }
        var deleted = false

        val ok = LegacyTokenMigration.run(true, { legacy }, { deleted = true }, KeystoreWrappedPreferences(InMemorySharedPreferences(), broken), TokenStorage.STORED_KEYS)

        assertFalse(ok)
        // A write-side failure keeps the old store for the next start (Fable m7).
        assertFalse(deleted)
    }

    @Test fun `a passing write failure is retried at the next start and then moves the session`() {
        val legacy = InMemorySharedPreferences().apply { edit().putString("access_token", "a").putString("refresh_token", "r").commit() }
        var failNow = true
        val flaky = object : ValueCipher {
            private val ok = testValueCipher()
            override fun encrypt(plain: ByteArray, aad: ByteArray): ByteArray =
                if (failNow) throw java.security.ProviderException("keystore busy") else ok.encrypt(plain, aad)
            override fun decrypt(blob: ByteArray, aad: ByteArray): ByteArray = ok.decrypt(blob, aad)
        }
        val store = KeystoreWrappedPreferences(InMemorySharedPreferences(), flaky)
        var deleted = false
        var failedWrites = 0

        assertFalse(LegacyTokenMigration.run(true, { legacy }, { deleted = true }, store, TokenStorage.STORED_KEYS, failedWrites) { failedWrites = it })
        assertFalse(deleted)
        assertEquals(1, failedWrites)

        failNow = false // the next start
        assertTrue(LegacyTokenMigration.run(true, { legacy }, { deleted = true }, store, TokenStorage.STORED_KEYS, failedWrites) { failedWrites = it })
        assertTrue(deleted)
        assertEquals(0, failedWrites)
        assertEquals("a", TokenStorage(store).getAccessToken())
    }

    @Test fun `the retry is bounded`() {
        val legacy = InMemorySharedPreferences().apply { edit().putString("access_token", "a").commit() }
        val broken = object : ValueCipher {
            override fun encrypt(plain: ByteArray, aad: ByteArray): ByteArray = throw java.security.ProviderException("keystore")
            override fun decrypt(blob: ByteArray, aad: ByteArray): ByteArray = throw java.security.ProviderException("keystore")
        }
        val store = KeystoreWrappedPreferences(InMemorySharedPreferences(), broken)
        var deleted = false
        var failedWrites = 0
        repeat(LegacyTokenMigration.MAX_WRITE_ATTEMPTS) {
            LegacyTokenMigration.run(true, { legacy }, { deleted = true }, store, TokenStorage.STORED_KEYS, failedWrites) { failedWrites = it }
        }
        assertTrue(deleted)
        assertEquals(0, failedWrites)
    }

    @Test fun `old OIDC verifiers are not moved`() {
        val legacy = InMemorySharedPreferences().apply {
            edit().putString("access_token", "a").putString("oidc_code_verifier", "v").putString("oidc_state", "s").commit()
        }
        assertTrue(LegacyTokenMigration.run(true, { legacy }, {}, target, TokenStorage.STORED_KEYS))
        assertFalse(target.contains("oidc_code_verifier"))
        assertFalse(target.contains("oidc_state"))
    }

    @Test fun `a legacy store left behind never overwrites tokens already moved`() {
        // The move committed, then the app died before the old file was deleted; the session has
        // rotated its single-use refresh token since (C27 SESSION_REUSED if the old one came back).
        TokenStorage(target).apply {
            saveServerUrl("https://chat.example.com")
            saveTokens("rotated-access", "rotated-refresh", 900, "u")
        }
        val legacy = InMemorySharedPreferences().apply {
            edit().putString("access_token", "old-access").putString("refresh_token", "old-refresh").putString("user_id", "u").commit()
        }
        var deleted = false

        assertTrue(LegacyTokenMigration.run(true, { legacy }, { deleted = true }, target, TokenStorage.STORED_KEYS))

        assertTrue(deleted)
        assertEquals("rotated-access", TokenStorage(target).getAccessToken())
        assertEquals("rotated-refresh", TokenStorage(target).getRefreshToken())
    }

    @Test fun `a failed move is void once the person signs in and out before the next start`() {
        val legacy = InMemorySharedPreferences().apply {
            edit().putString("access_token", "old-access").putString("refresh_token", "old-refresh").putString("user_id", "u1").commit()
        }
        var failNow = true
        val flaky = object : ValueCipher {
            private val ok = testValueCipher()
            override fun encrypt(plain: ByteArray, aad: ByteArray): ByteArray =
                if (failNow) throw java.security.ProviderException("keystore busy") else ok.encrypt(plain, aad)
            override fun decrypt(blob: ByteArray, aad: ByteArray): ByteArray = ok.decrypt(blob, aad)
        }
        val store = KeystoreWrappedPreferences(InMemorySharedPreferences(), flaky)
        var legacyExists = true
        var failedWrites = 0
        fun start() = LegacyTokenMigration.run(legacyExists, { legacy }, { legacyExists = false }, store, TokenStorage.STORED_KEYS, failedWrites) { failedWrites = it }

        assertFalse(start()) // the write failed: the old store is kept for the next start
        assertTrue(legacyExists)

        // As StorageModule wires it: a sign-in or logout voids the pending move.
        val storage = TokenStorage(store, null) {
            LegacyTokenMigration.discardPending(legacyExists, { legacyExists = false }, failedWrites) { failedWrites = it }
        }
        failNow = false // the Keystore works again
        storage.saveTokens("new-access", "new-refresh", 900, "u2") // the person signs in
        storage.clear() // and out again

        assertTrue(start()) // the next start: nothing is moved in
        assertFalse(legacyExists)
        assertEquals(0, failedWrites)
        assertNull(TokenStorage(store).getAccessToken())
        assertNull(TokenStorage(store).getRefreshToken())
    }

    @Test fun `a failed sign-in that clears an empty store keeps the pending move`() {
        val legacy = InMemorySharedPreferences().apply {
            edit().putString("access_token", "old-access").putString("refresh_token", "old-refresh").putString("user_id", "u1").commit()
        }
        var failNow = true
        val flaky = object : ValueCipher {
            private val ok = testValueCipher()
            override fun encrypt(plain: ByteArray, aad: ByteArray): ByteArray =
                if (failNow) throw java.security.ProviderException("keystore busy") else ok.encrypt(plain, aad)
            override fun decrypt(blob: ByteArray, aad: ByteArray): ByteArray = ok.decrypt(blob, aad)
        }
        val store = KeystoreWrappedPreferences(InMemorySharedPreferences(), flaky)
        var legacyExists = true
        var failedWrites = 0
        fun start() = LegacyTokenMigration.run(legacyExists, { legacy }, { legacyExists = false }, store, TokenStorage.STORED_KEYS, failedWrites) { failedWrites = it }

        assertFalse(start()) // the write failed: the old store is kept for the next start

        val storage = TokenStorage(store, null) {
            LegacyTokenMigration.discardPending(legacyExists, { legacyExists = false }, failedWrites) { failedWrites = it }
        }
        storage.clear() // a sign-in attempt failed before any session was stored: cleanup only

        assertTrue(legacyExists)
        failNow = false // the Keystore works again
        assertTrue(start()) // the next start moves the old session in
        assertEquals("old-access", TokenStorage(store).getAccessToken())
    }
}
