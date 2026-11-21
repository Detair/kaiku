package io.wolftown.kaiku.ui.dm

import androidx.annotation.StringRes
import io.ktor.http.HttpStatusCode
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.repository.PeopleUnavailableException
import java.io.IOException

/** User text for "New message" failures by type, status and code (C16); server text is never shown. */
object DmErrorText {

    /**
     * Opening a conversation. The server answers `VALIDATION_ERROR` alike for
     * "blocked, either way" and "no such account"; 403/404 mean the same to
     * the user. All get one neutral sentence that reveals neither.
     */
    @StringRes
    fun open(e: Throwable): Int = when {
        // The person could not be checked (no member list, or not every one): try again.
        e is PeopleUnavailableException -> common(underlying(e)) ?: R.string.dm_new_create_failed
        e is ApiException && (e.normalizedCode == "VALIDATION_ERROR" ||
            e.status == HttpStatusCode.Forbidden || e.status == HttpStatusCode.NotFound) -> R.string.dm_new_not_possible
        else -> common(e) ?: R.string.dm_new_create_failed
    }

    /** Loading the people list. */
    @StringRes
    fun load(e: Throwable): Int = common(underlying(e)) ?: R.string.dm_new_load_failed

    /** The failure behind [PeopleUnavailableException] (coroutine stack-trace recovery may wrap it twice). */
    private fun underlying(e: Throwable): Throwable {
        var current = e
        while (current is PeopleUnavailableException) current = current.cause ?: return current
        return current
    }

    @StringRes
    private fun common(e: Throwable): Int? = when {
        e is ApiException && e.status == HttpStatusCode.TooManyRequests -> R.string.errors_rate_limited
        e is ApiException && e.status.value >= 500 -> R.string.errors_server
        e is IOException -> R.string.errors_network
        else -> null
    }
}
