package io.wolftown.kaiku.approval

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.longOrNull
import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.util.Base64

/** C26 kinds: JSON `kind` and the signed kind byte. */
enum class SigKind(val byte: Int, val wire: String) {
    LOGIN(1, "login"),
    ENROLL(2, "enroll"),
    DEVICE_LOGIN(3, "device_login"),
    DEVICE_STEPUP(4, "device_stepup"),
    ENROLL_POP(5, "enroll_pop");

    companion object {
        /** Null for every byte v1.0 does not know, including the reserved 0x10–0x1F agent kinds. */
        fun fromByte(b: Int): SigKind? = entries.firstOrNull { it.byte == b }
    }
}

/** Signed decisions. Deny (0x02) is reserved and never signed in v1.0: "No" is the unsigned deny route. */
enum class Decision(val byte: Int, val wire: String) { APPROVE(1, "approve"), REPORT(3, "report") }

enum class LoginVariant(val byte: Int, val wire: String) { QR(1, "qr"), PASSWORD(2, "password"), OIDC(3, "oidc") }
enum class ClientClass(val byte: Int, val wire: String) { WEB(1, "web"), DESKTOP(2, "desktop"), ANDROID(3, "android") }
enum class EnrollVariant(val byte: Int, val wire: String) { OWN(1, "own"), ADMIN(2, "admin") }
enum class AuthenticatorType(val byte: Int, val wire: String) { KEY_DEVICE(1, "key_device"), PASSKEY(2, "passkey") }
enum class ClaimantCheck(val byte: Int, val wire: String) { NONE(0, "none"), IN_PERSON(1, "in_person"), CALLED_BACK(2, "called_back") }

/** A malformed prefix, context or encoding: the device refuses before any prompt. */
class SigFormatException(message: String) : Exception(message)

/**
 * A canonical context C (C26, field order binding). [canonical] is the exact
 * byte string that goes into `context_hash`; strings are their UTF-8 bytes as
 * received in JSON, never normalised or trimmed.
 */
sealed interface SigContext {
    val kind: SigKind
    fun canonical(): ByteArray

    companion object {
        /** Parses the JSON `context` of a challenge of [kind]; any missing, mistyped or unknown value throws. */
        fun fromJson(kind: SigKind, json: JsonObject): SigContext {
            val r = Fields(json)
            return when (kind) {
                SigKind.LOGIN -> LoginContext(
                    attemptId = r.uuid("attempt_id"),
                    collectionHash = r.hash("collection_hash"),
                    variant = r.enum("variant", LoginVariant.entries) { it.wire },
                    clientClass = r.enum("client_class", ClientClass.entries) { it.wire },
                    clientLabel = r.string("client_label"),
                    createdAt = r.long("created_at"),
                    serverName = r.string("server_name"),
                    place = r.string("place"),
                    username = r.string("username"),
                )
                SigKind.ENROLL -> EnrollContext(
                    enrollmentId = r.uuid("enrollment_id"),
                    subjectId = r.uuid("subject_id"),
                    subjectUsername = r.string("subject_username"),
                    variant = r.enum("variant", EnrollVariant.entries) { it.wire },
                    authenticatorType = r.enum("authenticator_type", AuthenticatorType.entries) { it.wire },
                    newKeyHash = r.hash("new_key_hash"),
                    label = r.string("label"),
                    claimantCheck = r.enum("claimant_check", ClaimantCheck.entries) { it.wire },
                )
                SigKind.DEVICE_LOGIN -> DeviceLoginContext(
                    clientClass = r.enum("client_class", ClientClass.entries) { it.wire },
                )
                SigKind.DEVICE_STEPUP -> DeviceStepUpContext(
                    sid = r.uuid("sid"),
                    action = r.string("action"),
                    target = r.string("target"),
                    targetLabel = r.string("target_label"),
                )
                SigKind.ENROLL_POP -> EnrollPopContext(
                    enrollmentId = r.uuid("enrollment_id"),
                    newKeyHash = r.hash("new_key_hash"),
                    attestationChallenge = r.hash("attestation_challenge"),
                    subjectUsername = r.string("subject_username"),
                )
            }
        }
    }
}

data class LoginContext(
    val attemptId: String,
    val collectionHash: ByteArray,
    val variant: LoginVariant,
    val clientClass: ClientClass,
    val clientLabel: String,
    /** Unix seconds of the attempt's creation. */
    val createdAt: Long,
    val serverName: String,
    val place: String,
    val username: String,
) : SigContext {
    override val kind get() = SigKind.LOGIN
    override fun canonical(): ByteArray = Canon()
        .uuid(attemptId).raw(collectionHash).byte(variant.byte).byte(clientClass.byte).str(clientLabel)
        .i64(createdAt).str(serverName).str(place).str(username).bytes()

    override fun equals(other: Any?) = other is LoginContext && canonical().contentEquals(other.canonical())
    override fun hashCode() = canonical().contentHashCode()
}

data class EnrollContext(
    val enrollmentId: String,
    val subjectId: String,
    val subjectUsername: String,
    val variant: EnrollVariant,
    val authenticatorType: AuthenticatorType,
    val newKeyHash: ByteArray,
    val label: String,
    val claimantCheck: ClaimantCheck,
) : SigContext {
    override val kind get() = SigKind.ENROLL
    override fun canonical(): ByteArray = Canon()
        .uuid(enrollmentId).uuid(subjectId).str(subjectUsername).byte(variant.byte).byte(authenticatorType.byte)
        .raw(newKeyHash).str(label).byte(claimantCheck.byte).bytes()

    override fun equals(other: Any?) = other is EnrollContext && canonical().contentEquals(other.canonical())
    override fun hashCode() = canonical().contentHashCode()
}

data class DeviceLoginContext(val clientClass: ClientClass) : SigContext {
    override val kind get() = SigKind.DEVICE_LOGIN
    override fun canonical(): ByteArray = Canon().byte(clientClass.byte).bytes()
}

/** `target_hash` = SHA-256(lp(target)) is computed here from the JSON [target]. */
data class DeviceStepUpContext(val sid: String, val action: String, val target: String, val targetLabel: String) : SigContext {
    override val kind get() = SigKind.DEVICE_STEPUP
    override fun canonical(): ByteArray = Canon()
        .uuid(sid).str(action).raw(KaikuSig.sha256(KaikuSig.lp(target.toByteArray(Charsets.UTF_8)))).str(targetLabel).bytes()
}

data class EnrollPopContext(
    val enrollmentId: String,
    val newKeyHash: ByteArray,
    val attestationChallenge: ByteArray,
    val subjectUsername: String,
) : SigContext {
    override val kind get() = SigKind.ENROLL_POP
    override fun canonical(): ByteArray = Canon()
        .uuid(enrollmentId).raw(newKeyHash).raw(attestationChallenge).str(subjectUsername).bytes()

    override fun equals(other: Any?) = other is EnrollPopContext && canonical().contentEquals(other.canonical())
    override fun hashCode() = canonical().contentHashCode()
}

/** Builder for the C26 encodings (lp strings, raw UUIDs, i64, single bytes). */
private class Canon {
    private val out = ByteArrayOutputStream()
    fun uuid(id: String) = apply { out.write(KaikuSig.uuidBytes(id)) }
    fun raw(b: ByteArray) = apply { out.write(b) }
    fun byte(b: Int) = apply { out.write(b) }
    fun str(s: String) = apply { out.write(KaikuSig.lp(s.toByteArray(Charsets.UTF_8))) }
    fun i64(v: Long) = apply { out.write(ByteBuffer.allocate(8).putLong(v).array()) }
    fun bytes(): ByteArray = out.toByteArray()
}

/** Strict typed reads from a context object. */
private class Fields(private val json: JsonObject) {
    private fun prim(name: String): JsonPrimitive =
        json[name] as? JsonPrimitive ?: throw SigFormatException("context field $name missing")

    fun string(name: String): String {
        val p = prim(name)
        if (!p.isString) throw SigFormatException("context field $name is not a string")
        return p.content
    }

    fun long(name: String): Long {
        val p = prim(name)
        if (p.isString) throw SigFormatException("context field $name is not a number")
        return p.longOrNull ?: throw SigFormatException("context field $name is not an integer")
    }

    fun uuid(name: String): String = string(name).also { KaikuSig.uuidBytes(it) }

    fun hash(name: String): ByteArray {
        val b = try {
            Base64.getUrlDecoder().decode(string(name))
        } catch (_: IllegalArgumentException) {
            throw SigFormatException("context field $name is not base64url")
        }
        if (b.size != 32) throw SigFormatException("context field $name is not 32 bytes")
        return b
    }

    fun <T> enum(name: String, values: List<T>, wire: (T) -> String): T {
        val raw = string(name)
        return values.firstOrNull { wire(it) == raw } ?: throw SigFormatException("context field $name has an unknown value")
    }
}
