package io.wolftown.kaiku.approval

import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.security.MessageDigest

/** The fields of a C26 signing prefix (M up to and including `context_hash`). */
data class SigningPrefix(
    val kind: Int,
    val challengeId: String,
    val nonce: ByteArray,
    val expiresAt: Long,
    val origin: String,
    val userId: String,
    val deviceId: String,
    val contextHash: ByteArray,
)

/**
 * C26 signed bytes:
 * `M = "kaiku-sig-v1" ‖ kind ‖ challenge_id ‖ nonce[32] ‖ i64(expires_at) ‖ lp(origin) ‖ user_id ‖ device_id
 *      ‖ context_hash ‖ decision ‖ lp(response)`, `context_hash = SHA-256("kaiku-sig-context-v1" ‖ kind ‖ C)`.
 * The device appends `decision ‖ lp(response)` to the server's prefix and signs M whole.
 */
object KaikuSig {
    const val MAGIC = "kaiku-sig-v1"
    const val CONTEXT_MAGIC = "kaiku-sig-context-v1"

    private const val NONCE_LEN = 32
    private const val HASH_LEN = 32
    private const val UUID_LEN = 16
    private val UUID_RE = Regex("^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")

    /** u32 big-endian byte length ‖ bytes. */
    fun lp(bytes: ByteArray): ByteArray =
        ByteBuffer.allocate(4 + bytes.size).putInt(bytes.size).put(bytes).array()

    /** The 16 bytes of a lowercase hyphenated UUID in RFC 9562 order (the hex digits of the string). */
    fun uuidBytes(id: String): ByteArray {
        if (!UUID_RE.matches(id)) throw SigFormatException("not a lowercase UUID")
        val hex = id.replace("-", "")
        return ByteArray(UUID_LEN) { hex.substring(2 * it, 2 * it + 2).toInt(16).toByte() }
    }

    private fun uuidString(b: ByteArray, from: Int): String {
        val hex = (from until from + UUID_LEN).joinToString("") { "%02x".format(b[it]) }
        return "${hex.substring(0, 8)}-${hex.substring(8, 12)}-${hex.substring(12, 16)}-${hex.substring(16, 20)}-${hex.substring(20)}"
    }

    fun sha256(b: ByteArray): ByteArray = MessageDigest.getInstance("SHA-256").digest(b)

    /** SHA-256(CONTEXT_MAGIC ‖ kind ‖ C). */
    fun contextHash(ctx: SigContext): ByteArray {
        val out = ByteArrayOutputStream()
        out.write(CONTEXT_MAGIC.toByteArray(Charsets.US_ASCII))
        out.write(ctx.kind.byte)
        out.write(ctx.canonical())
        return sha256(out.toByteArray())
    }

    /** True when [prefix] starts with the v1 magic. */
    fun hasMagic(prefix: ByteArray): Boolean {
        val magic = MAGIC.toByteArray(Charsets.US_ASCII)
        return prefix.size >= magic.size && prefix.copyOfRange(0, magic.size).contentEquals(magic)
    }

    /**
     * Splits a prefix by the C26 layout: magic [0,12), kind [12], challenge_id [13,29), nonce [29,61),
     * expires_at [61,69), origin length [69,73), origin, user_id, device_id, context_hash. Anything
     * shorter, longer, without the magic or with a non-ASCII origin throws [SigFormatException].
     */
    fun parsePrefix(prefix: ByteArray): SigningPrefix {
        if (!hasMagic(prefix)) throw SigFormatException("magic")
        var at = MAGIC.length
        fun need(n: Int) { if (prefix.size < at + n) throw SigFormatException("prefix too short") }
        need(1)
        val kind = prefix[at].toInt() and 0xff
        at += 1
        need(UUID_LEN)
        val challengeId = uuidString(prefix, at)
        at += UUID_LEN
        need(NONCE_LEN)
        val nonce = prefix.copyOfRange(at, at + NONCE_LEN)
        at += NONCE_LEN
        need(8)
        val expiresAt = ByteBuffer.wrap(prefix, at, 8).long
        at += 8
        need(4)
        val originLen = ByteBuffer.wrap(prefix, at, 4).int
        at += 4
        if (originLen < 0 || originLen > prefix.size) throw SigFormatException("origin length")
        need(originLen)
        val originBytes = prefix.copyOfRange(at, at + originLen)
        if (originBytes.any { it < 0x20 || it > 0x7e }) throw SigFormatException("origin is not ASCII")
        val origin = String(originBytes, Charsets.US_ASCII)
        at += originLen
        need(UUID_LEN)
        val userId = uuidString(prefix, at)
        at += UUID_LEN
        need(UUID_LEN)
        val deviceId = uuidString(prefix, at)
        at += UUID_LEN
        need(HASH_LEN)
        val contextHash = prefix.copyOfRange(at, at + HASH_LEN)
        at += HASH_LEN
        if (at != prefix.size) throw SigFormatException("trailing bytes after context_hash")
        return SigningPrefix(kind, challengeId, nonce, expiresAt, origin, userId, deviceId, contextHash)
    }

    /** prefix ‖ decision ‖ lp(response): the exact bytes the key signs. */
    fun message(prefix: ByteArray, decision: Decision, response: ByteArray): ByteArray {
        val out = ByteArrayOutputStream(prefix.size + 5 + response.size)
        out.write(prefix)
        out.write(decision.byte)
        out.write(lp(response))
        return out.toByteArray()
    }
}
