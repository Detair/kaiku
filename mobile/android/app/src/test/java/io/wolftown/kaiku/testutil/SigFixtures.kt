package io.wolftown.kaiku.testutil

import io.wolftown.kaiku.approval.KaikuSig
import io.wolftown.kaiku.approval.SigContext
import io.wolftown.kaiku.approval.SigKind
import io.wolftown.kaiku.domain.model.ApprovalChallenge
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.security.KeyFactory
import java.security.PublicKey
import java.security.Signature
import java.security.spec.X509EncodedKeySpec
import java.util.Base64

/**
 * C26 golden-vector inputs (contracts file, "Golden vectors (normative)") and
 * helpers to build signing prefixes the way the server does. Test-only: the app
 * never builds a prefix, it only parses and checks one.
 */
object SigFixtures {
    const val PUBLIC_KEY =
        "MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEaGyjIg4bpThSbH_2lxjxOCbet8d6OXzr6dvBwmYvZkX9zHpvoOqY5gJuY5LHJQSE3ny7KokI1Ds-0iPwEDhAZQ"
    const val ORIGIN = "https://chat.example.com"
    const val EXPIRES_AT = 1793707320L
    const val USER_ID = "11111111-1111-4111-8111-111111111111"
    const val DEVICE_ID = "22222222-2222-4222-8222-222222222222"
    val NONCE: ByteArray = ByteArray(32) { it.toByte() }

    fun challengeId(n: Int): String = "33333333-3333-4333-8333-3333333333%02d".format(n)

    fun json(s: String): JsonObject = Json.parseToJsonElement(s).jsonObject

    fun ByteArray.hex(): String = joinToString("") { "%02x".format(it) }

    fun unhex(s: String): ByteArray = ByteArray(s.length / 2) { s.substring(2 * it, 2 * it + 2).toInt(16).toByte() }

    fun b64u(b: ByteArray): String = Base64.getUrlEncoder().withoutPadding().encodeToString(b)

    fun unb64u(s: String): ByteArray = Base64.getUrlDecoder().decode(s)

    fun bytes(range: IntRange): ByteArray = range.map { it.toByte() }.toByteArray()

    /** M up to and including context_hash, laid out as C26 "Prefix layout". */
    fun prefixFor(
        kind: SigKind,
        challengeId: String,
        ctx: SigContext,
        userId: String = USER_ID,
        deviceId: String = DEVICE_ID,
        origin: String = ORIGIN,
        expiresAt: Long = EXPIRES_AT,
        kindByte: Int = kind.byte,
        contextHash: ByteArray = KaikuSig.contextHash(ctx),
        magic: String = KaikuSig.MAGIC,
    ): ByteArray {
        val out = ByteArrayOutputStream()
        out.write(magic.toByteArray(Charsets.US_ASCII))
        out.write(kindByte)
        out.write(KaikuSig.uuidBytes(challengeId))
        out.write(NONCE)
        out.write(ByteBuffer.allocate(8).putLong(expiresAt).array())
        out.write(KaikuSig.lp(origin.toByteArray(Charsets.US_ASCII)))
        out.write(KaikuSig.uuidBytes(userId))
        out.write(KaikuSig.uuidBytes(deviceId))
        out.write(contextHash)
        return out.toByteArray()
    }

    fun challengeFor(
        kind: SigKind,
        n: Int,
        contextJson: String,
        variant: String? = null,
        prefix: ByteArray? = null,
        wireKind: String = kind.wire,
    ): ApprovalChallenge {
        val ctx = SigContext.fromJson(kind, json(contextJson))
        val p = prefix ?: prefixFor(kind, challengeId(n), ctx)
        return ApprovalChallenge(
            challengeId = challengeId(n),
            kind = wireKind,
            variant = variant,
            expiresAt = "2026-11-03T12:02:00Z",
            signingPrefix = b64u(p),
            context = json(contextJson),
        )
    }

    fun publicKey(spkiB64u: String = PUBLIC_KEY): PublicKey =
        KeyFactory.getInstance("EC").generatePublic(X509EncodedKeySpec(unb64u(spkiB64u)))

    /** java.security ECDSA verification; accepts high and low s, as C26 requires. */
    fun verifyP256(spkiB64u: String, message: ByteArray, signatureB64u: String): Boolean =
        Signature.getInstance("SHA256withECDSA").run {
            initVerify(publicKey(spkiB64u))
            update(message)
            verify(unb64u(signatureB64u))
        }

    fun sha256(b: ByteArray): ByteArray = java.security.MessageDigest.getInstance("SHA-256").digest(b)

    // --- C26 vectors V01–V05: context as sent ---
    const val V01_CONTEXT =
        """{"attempt_id":"44444444-4444-4444-8444-444444444444","collection_hash":"E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM","variant":"qr","client_class":"desktop","client_label":"Kaiku desktop on Windows","created_at":1793707200,"server_name":"Familie Müller","place":"Hamburg, DE","username":"alice"}"""
    const val V02_CONTEXT =
        """{"enrollment_id":"55555555-5555-4555-8555-555555555555","subject_id":"66666666-6666-4666-8666-666666666666","subject_username":"oma","variant":"admin","authenticator_type":"key_device","new_key_hash":"Bs5H8qw80BPj7K2nOZOOcMYlOZs6KhOHt1xIDKHM_9Q","label":"Pixel 8","claimant_check":"called_back"}"""
    const val V03_CONTEXT = """{"client_class":"android"}"""
    const val V04_CONTEXT =
        """{"sid":"77777777-7777-4777-8777-777777777777","action":"key_device.remove","target":"99999999-9999-4999-8999-999999999999","target_label":"Pixel 8"}"""
    const val V05_CONTEXT =
        """{"enrollment_id":"55555555-5555-4555-8555-555555555555","new_key_hash":"SNXZ3WXZTk1gg0TCFd1P6gmdzP1Rwao9hsqXg-SsX10","attestation_challenge":"QEFCQ0RFRkdISUpLTE1OT1BRUlNUVVZXWFlaW1xdXl8","subject_username":"oma"}"""
}
