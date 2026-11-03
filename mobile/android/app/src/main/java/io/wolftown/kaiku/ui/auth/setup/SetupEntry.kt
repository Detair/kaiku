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

    override fun toString(): String = "SetupEntry(origin=$origin, source=$source)"
}
