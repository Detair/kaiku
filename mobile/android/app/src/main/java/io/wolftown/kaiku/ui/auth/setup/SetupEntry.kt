package io.wolftown.kaiku.ui.auth.setup

/**
 * A setup code to act on: scanned, opened as a link, or issued for this app's
 * own registration ([ticket], F8). [tokenB64u] and [ticket] are secrets.
 */
data class SetupEntry(
    val origin: String,
    val tokenB64u: String,
    val source: Source,
    val ticket: String? = null,
) {
    enum class Source { SCAN, LINK, TICKET }

    /**
     * The same setup code on the same server, however it arrived (scanned, or opened as a link):
     * one enrolment is one link.
     */
    fun sameCodeAs(other: SetupEntry?): Boolean = other != null && tokenB64u == other.tokenB64u &&
        (origin == other.origin || io.wolftown.kaiku.domain.KaikuPayloads.canonicalOrigin(origin)
            ?.let { it == io.wolftown.kaiku.domain.KaikuPayloads.canonicalOrigin(other.origin) } == true)

    override fun toString(): String = "SetupEntry(origin=$origin, source=$source)"
}
