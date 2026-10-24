package io.wolftown.kaiku.data.api

import io.wolftown.kaiku.data.local.TokenStorage
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.Interceptor
import okhttp3.Response

/**
 * Adds `Authorization: Bearer …` to requests that go to the configured Kaiku
 * server, so the image loader can fetch protected files.
 *
 * The header is only added when the request has the same origin as the
 * configured server URL: both https, same host (case-insensitive) and same
 * port (443 when not given). Requests to any other host, port or scheme are
 * passed on unchanged, and a request that already carries an
 * `Authorization` header is never modified.
 *
 * Register it with `addInterceptor` (application interceptor), not as a
 * network interceptor: it then sees only the original request, and OkHttp
 * drops the `Authorization` header itself when following a redirect to a
 * different host.
 *
 * Never logs the token or the request URL.
 */
class AuthHeaderInterceptor(private val tokenStorage: TokenStorage) : Interceptor {

    override fun intercept(chain: Interceptor.Chain): Response {
        val request = chain.request()
        if (request.header(AUTHORIZATION) != null) return chain.proceed(request)

        val server = tokenStorage.getServerUrl()?.toHttpUrlOrNull()
            ?: return chain.proceed(request)
        if (!isSameHttpsOrigin(request.url, server)) return chain.proceed(request)

        val token = tokenStorage.getAccessToken()
        if (token.isNullOrEmpty()) return chain.proceed(request)

        return chain.proceed(
            request.newBuilder()
                .header(AUTHORIZATION, "Bearer $token")
                .build()
        )
    }

    private companion object {
        const val AUTHORIZATION = "Authorization"

        fun isSameHttpsOrigin(url: HttpUrl, server: HttpUrl): Boolean =
            url.isHttps &&
                server.isHttps &&
                url.host.equals(server.host, ignoreCase = true) &&
                url.port == server.port
    }
}
