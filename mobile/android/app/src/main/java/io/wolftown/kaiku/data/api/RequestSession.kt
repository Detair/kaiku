package io.wolftown.kaiku.data.api

import io.ktor.client.request.HttpRequestBuilder
import kotlin.coroutines.AbstractCoroutineContextElement
import kotlin.coroutines.CoroutineContext
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.withContext

/**
 * The signed-in session (`AuthSession.LoggedIn.sessionId`) the requests of
 * a coroutine are built for. The APIs that read it ([DmApi], the member
 * lists of [GuildApi]) bind each request to that session through the HTTP
 * client's required-session check: once another session, or none, is the
 * current one, the request is not sent ([SessionChangedException]) and so
 * never leaves with another account's token.
 */
class RequestSession(val id: Long) : AbstractCoroutineContextElement(Key) {
    companion object Key : CoroutineContext.Key<RequestSession>
}

/** Runs [block] with its requests bound to the session [id]. */
suspend fun <T> withRequestSession(id: Long, block: suspend () -> T): T = withContext(RequestSession(id)) { block() }

/** The session the calling coroutine's requests are bound to, or null. */
internal suspend fun boundSession(): Long? = currentCoroutineContext()[RequestSession]?.id

/** Binds this request to [session] (from [boundSession]), if there is one. */
internal fun HttpRequestBuilder.requireSession(session: Long?) {
    if (session != null) attributes.put(KaikuHttpClient.RequiredSessionId, session)
}
