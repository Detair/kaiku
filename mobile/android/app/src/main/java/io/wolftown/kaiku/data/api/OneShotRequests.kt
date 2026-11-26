package io.wolftown.kaiku.data.api

import io.ktor.client.request.HttpRequestBuilder
import io.ktor.client.request.header
import okhttp3.Interceptor
import okhttp3.Response
import java.io.IOException
import java.util.Collections
import java.util.UUID

/**
 * Requests that must reach the server at most once (a signed claim, a signed answer, a device
 * sign-in, a registration): the HTTP stack may send a request again by itself (after a dropped
 * connection, a `408`, or a `503` that says "again now"). Such a request is marked
 * ([HttpRequestBuilder.sentOnce]); this network interceptor lets each marked request onto the
 * wire once and fails every further attempt of it, so a repeat never reaches the server. The mark
 * itself never leaves the phone.
 */
class OneShotRequests : Interceptor {
    companion object {
        internal const val HEADER = "X-Kaiku-Send-Once"
        /** Marks remembered; older ones belong to requests long finished. */
        private const val REMEMBERED = 256
    }

    private val seen: MutableSet<String> = Collections.synchronizedSet(
        Collections.newSetFromMap(object : LinkedHashMap<String, Boolean>() {
            override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, Boolean>?) = size > REMEMBERED
        }),
    )

    override fun intercept(chain: Interceptor.Chain): Response {
        val request = chain.request()
        val mark = request.header(HEADER) ?: return chain.proceed(request)
        if (!seen.add(mark)) throw IOException("a request that is sent once was not sent again")
        return chain.proceed(request.newBuilder().removeHeader(HEADER).build())
    }
}

/** This request reaches the server at most once ([OneShotRequests]). */
internal fun HttpRequestBuilder.sentOnce() {
    header(OneShotRequests.HEADER, UUID.randomUUID().toString())
}
