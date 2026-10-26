package io.wolftown.kaiku.data.local

import io.wolftown.kaiku.testutil.InMemorySharedPreferences
import io.wolftown.kaiku.testutil.ThrowingReadPreferences
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test

class TokenStorageTest {

    private lateinit var storage: TokenStorage

    @Before
    fun setUp() {
        storage = TokenStorage(InMemorySharedPreferences())
        storage.saveTokens("access-1", "refresh-1", 900, "user-1")
    }

    @Test
    fun `conditional save writes nothing when stored refresh token changed`() {
        storage.saveTokens("access-2", "refresh-2", 900, "user-2")
        val expiresAtBefore = storage.getAccessTokenExpiresAtMillis()

        val saved = storage.saveTokensIfRefreshTokenIs(
            expectedRefreshToken = "refresh-1",
            accessToken = "rotated-access",
            refreshToken = "rotated-refresh",
            expiresIn = 3600,
            userId = "user-1"
        )

        assertFalse(saved)
        assertEquals("access-2", storage.getAccessToken())
        assertEquals("refresh-2", storage.getRefreshToken())
        assertEquals("user-2", storage.getUserId())
        assertEquals(expiresAtBefore, storage.getAccessTokenExpiresAtMillis())
    }

    @Test
    fun `conditional save writes nothing after clear`() {
        storage.clear()

        val saved = storage.saveTokensIfRefreshTokenIs("refresh-1", "rotated-access", "rotated-refresh", 900, "user-1")

        assertFalse(saved)
        assertNull(storage.getAccessToken())
        assertNull(storage.getRefreshToken())
    }

    @Test
    fun `conditional save rotates tokens and keeps the session generation`() {
        val generation = storage.getSessionGeneration()

        val saved = storage.saveTokensIfRefreshTokenIs("refresh-1", "rotated-access", "rotated-refresh", 900, "user-1")

        assertTrue(saved)
        assertEquals("rotated-access", storage.getAccessToken())
        assertEquals("rotated-refresh", storage.getRefreshToken())
        assertEquals(generation, storage.getSessionGeneration())
    }

    @Test
    fun `login logout and server change start a new session generation`() {
        val g0 = storage.getSessionGeneration()
        storage.saveTokens("access-2", "refresh-2", 900, "user-2")
        val g1 = storage.getSessionGeneration()
        storage.clear()
        val g2 = storage.getSessionGeneration()
        storage.saveServerUrl("https://other.example.com")
        val g3 = storage.getSessionGeneration()

        assertNotEquals(g0, g1)
        assertNotEquals(g1, g2)
        assertNotEquals(g2, g3)
    }

    @Test
    fun `conditional clear leaves a newer login untouched`() {
        storage.saveTokens("access-2", "refresh-2", 900, "user-2")

        assertFalse(storage.clearIfRefreshTokenIs("refresh-1"))
        assertEquals("refresh-2", storage.getRefreshToken())
        assertEquals("access-2", storage.getAccessToken())
    }

    @Test
    fun `conditional clear removes the matching session`() {
        assertTrue(storage.clearIfRefreshTokenIs("refresh-1"))
        assertNull(storage.getAccessToken())
        assertNull(storage.getRefreshToken())
        assertNull(storage.getUserId())
    }

    @Test
    fun `read failure resets storage and reports once`() {
        val prefs = ThrowingReadPreferences(InMemorySharedPreferences())
        val damaged = TokenStorage(prefs)
        damaged.saveTokens("access-1", "refresh-1", 900, "user-1")
        val generation = damaged.getSessionGeneration()
        assertFalse(damaged.consumeStorageResetNotice())

        prefs.failure = ThrowingReadPreferences.decryptFailure()
        assertNull(damaged.getAccessToken())
        prefs.failure = null

        assertNull(damaged.getAccessToken())
        assertNull(damaged.getRefreshToken())
        assertNotEquals(generation, damaged.getSessionGeneration())
        assertTrue(damaged.consumeStorageResetNotice())
        assertFalse(damaged.consumeStorageResetNotice())
    }

    @Test
    fun `read failure by security exception is handled and storage is usable again`() {
        val prefs = ThrowingReadPreferences(InMemorySharedPreferences())
        val damaged = TokenStorage(prefs)
        damaged.saveTokens("access-1", "refresh-1", 900, "user-1")

        prefs.failure = SecurityException("test")
        assertNull(damaged.getRefreshToken())
        prefs.failure = null

        damaged.saveTokens("access-2", "refresh-2", 900, "user-2")
        assertEquals("access-2", damaged.getAccessToken())
        assertTrue(damaged.consumeStorageResetNotice())
    }

    @Test
    fun `conditional writers fail closed when the read fails`() {
        val prefs = ThrowingReadPreferences(InMemorySharedPreferences())
        val damaged = TokenStorage(prefs)
        damaged.saveTokens("access-1", "refresh-1", 900, "user-1")

        prefs.failure = ThrowingReadPreferences.decryptFailure()
        val saved = damaged.saveTokensIfRefreshTokenIs("refresh-1", "rotated", "rotated-r", 900, "user-1")
        prefs.failure = null

        assertFalse(saved)
        assertNull(damaged.getAccessToken())
    }

    @Test
    fun `read that succeeds on retry does not wipe a committing login`() {
        val inner = InMemorySharedPreferences()
        val prefs = ThrowingReadPreferences(inner)
        val s = TokenStorage(prefs)
        s.saveTokens("old", "old-r", 900, "user-1")
        prefs.failure = ThrowingReadPreferences.decryptFailure()
        prefs.failures = 1
        // The login commits (without moving the generation we captured) before the reset runs.
        prefs.onFailure = { inner.edit().putString("access_token", "fresh").commit() }

        assertEquals("fresh", s.getAccessToken())

        assertEquals("old-r", s.getRefreshToken())
        assertFalse(s.consumeStorageResetNotice())
    }

    @Test
    fun `reset is skipped when the session changed while the failure was pending`() {
        val prefs = ThrowingReadPreferences(InMemorySharedPreferences())
        val s = TokenStorage(prefs)
        s.saveTokens("old", "old-r", 900, "user-1")
        prefs.failure = ThrowingReadPreferences.decryptFailure()
        prefs.failures = 1
        prefs.onFailure = { s.saveTokens("new", "new-r", 900, "user-2") }

        assertNull(s.getAccessToken())

        assertEquals("new", s.getAccessToken())
        assertEquals("user-2", s.getUserId())
        assertFalse(s.consumeStorageResetNotice())
    }

    @Test
    fun `io failure is handled like a security failure`() {
        val prefs = ThrowingReadPreferences(InMemorySharedPreferences())
        val s = TokenStorage(prefs)
        s.saveTokens("a", "r", 900, "user-1")

        prefs.failure = java.io.IOException("test")
        assertNull(s.getUserId())
        prefs.failure = null

        assertNull(s.getRefreshToken())
        assertTrue(s.consumeStorageResetNotice())
    }

    @Test
    fun `reset keeps the server address and clears pending sign-in state`() {
        val prefs = ThrowingReadPreferences(InMemorySharedPreferences())
        val s = TokenStorage(prefs)
        s.saveServerUrl("https://kaiku.example.com")
        s.saveTokens("a", "r", 900, "user-1")
        s.saveOidcPkceState("verifier", "state")

        prefs.failure = ThrowingReadPreferences.decryptFailure()
        prefs.failures = 2 // the failed read and its retry; later reads work
        assertNull(s.getAccessToken())

        assertEquals("https://kaiku.example.com", s.getServerUrl())
        assertNull(s.getOidcCodeVerifier())
        assertNull(s.getOidcState())
        assertNull(s.getRefreshToken())
    }

    @Test
    fun `reset notice survives a new instance until consumed`() {
        val notice = InMemorySharedPreferences()
        val prefs = ThrowingReadPreferences(InMemorySharedPreferences())
        val first = TokenStorage(prefs, notice)
        first.saveTokens("a", "r", 900, "user-1")
        prefs.failure = ThrowingReadPreferences.decryptFailure()
        assertNull(first.getAccessToken())
        prefs.failure = null

        val afterRestart = TokenStorage(prefs, notice)
        assertTrue(afterRestart.consumeStorageResetNotice())
        assertFalse(TokenStorage(prefs, notice).consumeStorageResetNotice())
    }
}
