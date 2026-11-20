package io.wolftown.kaiku.data.session

import io.wolftown.kaiku.data.local.AuthSession

/**
 * Who is signed in, for tests: a [SessionGeneration] over a session the test
 * sets. [signOut] and [signIn] do what the token store and the auth state do
 * in the app, in the same order (the hook first).
 */
class FakeSessions(user: String? = "me") {
    private var ids = 0L

    @Volatile
    var session: AuthSession = if (user != null) AuthSession.LoggedIn(user, ++ids) else AuthSession.LoggedOut

    /** The stored account of a process that has not restored its session. */
    @Volatile
    var stored: String? = null

    val generation = SessionGeneration({ session }, { stored })

    val sessionId: Long? get() = (session as? AuthSession.LoggedIn)?.sessionId

    /** The stored session is removed: the hook's advance, then the account state. */
    fun signOut() {
        generation.advance()
        stored = null
        session = AuthSession.LoggedOut
    }

    fun signIn(user: String) {
        session = AuthSession.LoggedIn(user, ++ids)
    }

    /** Sign-out and sign-in of [user] (another account, or the same one renewing its session). */
    fun switchTo(user: String) {
        signOut()
        signIn(user)
    }
}
