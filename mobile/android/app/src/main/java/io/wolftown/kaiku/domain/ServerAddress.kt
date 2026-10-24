package io.wolftown.kaiku.domain

/** Why [normalizeServerAddress] rejected an address. */
class InvalidServerAddressException(val reason: Reason) : IllegalArgumentException(reason.name) {
    enum class Reason { BLANK, NOT_SECURE, MALFORMED }
}

private val HOST_PART = Regex("^[A-Za-z0-9]([A-Za-z0-9-]*[A-Za-z0-9])?$")

/**
 * Turns what a person typed into a server base URL:
 * - trims; blank is rejected
 * - a bare host (`kaiku.example.com`) gets `https://`
 * - `http://` is rejected (and any other scheme)
 * - path, query, fragment and trailing slash are dropped
 *
 * The result is `https://host[:port]` with the host in lower case.
 */
fun normalizeServerAddress(input: String): Result<String> {
    val text = input.trim()
    if (text.isEmpty()) return fail(InvalidServerAddressException.Reason.BLANK)

    val withScheme = when {
        text.startsWith("https://", ignoreCase = true) -> text
        text.contains("://") -> return fail(InvalidServerAddressException.Reason.NOT_SECURE)
        else -> "https://$text"
    }

    val authority = withScheme.substring("https://".length)
        .takeWhile { it != '/' && it != '?' && it != '#' }
        .lowercase()
    if (authority.isEmpty() || authority.contains('@')) {
        return fail(InvalidServerAddressException.Reason.MALFORMED)
    }

    val colon = authority.lastIndexOf(':')
    // A trailing dot (fully qualified name) is dropped.
    val host = (if (colon >= 0) authority.substring(0, colon) else authority).removeSuffix(".")
    var portSuffix = ""
    if (colon >= 0) {
        val portText = authority.substring(colon + 1)
        val port = portText.takeIf { text -> text.isNotEmpty() && text.all { it in '0'..'9' } }
            ?.toIntOrNull()
        if (port == null || port !in 1..65535) {
            return fail(InvalidServerAddressException.Reason.MALFORMED)
        }
        portSuffix = ":$port"
    }
    if (host.isEmpty() || !host.split('.').all { HOST_PART.matches(it) }) {
        return fail(InvalidServerAddressException.Reason.MALFORMED)
    }
    return Result.success("https://$host$portSuffix")
}

private fun fail(reason: InvalidServerAddressException.Reason): Result<String> =
    Result.failure(InvalidServerAddressException(reason))
