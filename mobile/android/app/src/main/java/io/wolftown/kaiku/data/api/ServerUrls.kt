package io.wolftown.kaiku.data.api

/**
 * Turns the URLs the server sends (avatars, server icons, attachments) into
 * URLs the image loader can fetch.
 *
 * The server sends most file URLs as paths relative to itself
 * (`/api/files/...`), so they need the configured server URL in front.
 */
object ServerUrls {

    /**
     * Resolves [url] against [serverBaseUrl]:
     * - `null` or blank [url] → `null`
     * - `https://…` or `http://…` → returned unchanged
     * - a path starting with a single `/` → `serverBaseUrl.trimEnd('/') + url`
     * - anything else (including protocol-relative `//host/…`), or a
     *   relative path without a base → `null`
     */
    fun resolve(serverBaseUrl: String?, url: String?): String? {
        if (url.isNullOrBlank()) return null
        if (url.startsWith("https://", ignoreCase = true) ||
            url.startsWith("http://", ignoreCase = true)
        ) {
            return url
        }
        if (url.startsWith("/") && !url.startsWith("//")) {
            if (serverBaseUrl.isNullOrBlank()) return null
            return serverBaseUrl.trimEnd('/') + url
        }
        return null
    }
}

