package io.wolftown.kaiku.data.auth

import javax.inject.Inject
import javax.inject.Singleton

/**
 * The error code of the last refresh that ended the session (`SESSION_REUSED`,
 * `SESSION_EXPIRED`, …), so "Confirm it's you" can say why. Codes only.
 */
@Singleton
class SessionEndLog @Inject constructor() {
    @Volatile
    private var last: String? = null

    fun record(code: String?) {
        last = code
    }

    /** The recorded code, once. */
    fun consume(): String? = last.also { last = null }
}
