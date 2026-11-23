package io.wolftown.kaiku.approval

import io.wolftown.kaiku.domain.model.ApprovalChallenge
import io.wolftown.kaiku.testutil.SigFixtures
import io.wolftown.kaiku.testutil.SigFixtures.hex
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** The `pc_setup` (kind 6) vectors P01/P02 of Task 1, byte for byte as the server computes them. */
class PcSetupSigVectorsTest {
    private val file = Json.parseToJsonElement(
        javaClass.classLoader!!.getResource("contract/pc_setup_sig_vectors.json")!!.readText(),
    ).jsonObject
    private val vectors = file["vectors"]!!.jsonArray.map { it.jsonObject }
    private val publicKey = file["public_key"]!!.jsonPrimitive.content

    private fun JsonObject.str(key: String) = this[key]!!.jsonPrimitive.content

    @Test fun `there are an approve and a report vector`() {
        assertEquals(listOf("P01", "P02"), vectors.map { it.str("name") })
    }

    @Test fun `context bytes hash and signed message match the server`() {
        vectors.forEach { v ->
            val name = v.str("name")
            val ctx = SigContext.fromJson(SigKind.PC_SETUP, v["context"]!!.jsonObject)
            assertTrue(name, ctx is PcSetupContext)
            assertEquals(name, v.str("c_hex"), ctx.canonical().hex())
            assertEquals(name, v.str("context_hash_hex"), KaikuSig.contextHash(ctx).hex())
            val decision = Decision.entries.first { it.wire == v.str("decision") }
            val prefix = SigFixtures.unb64u(v.str("signing_prefix_b64u"))
            val m = KaikuSig.message(prefix, decision, SigFixtures.unhex(v.str("response_hex")))
            assertEquals(name, v.str("message_hex"), m.hex())
            assertEquals(6, m[12].toInt())
            assertTrue(name, SigFixtures.verifyP256(publicKey, m, v.str("signature")))
        }
    }

    @Test fun `the pre-UV checks accept both vectors`() {
        vectors.forEach { v ->
            val expiresAt = v["expires_at"]!!.jsonPrimitive.long
            val checks = ChallengeVerifier({ v.str("origin") }, { expiresAt - 30 })
            val ch = ApprovalChallenge(
                challengeId = v.str("challenge_id"), kind = v.str("kind"), expiresAt = "2026-11-03T12:02:00Z",
                signingPrefix = v.str("signing_prefix_b64u"), context = v["context"]!!.jsonObject,
            )
            val verified = checks.verify(ch, SigKind.PC_SETUP, v.str("user_id"), v.str("device_id"))
            assertEquals(v.str("challenge_id"), verified.challengeId)
        }
    }

    @Test fun `a changed context field fails the hash check`() {
        val v = vectors.first()
        val ctx = v["context"]!!.jsonObject
        val changed = JsonObject(ctx + ("client_description" to kotlinx.serialization.json.JsonPrimitive("Chrome on Linux")))
        val checks = ChallengeVerifier({ v.str("origin") }, { v["expires_at"]!!.jsonPrimitive.long - 30 })
        val ch = ApprovalChallenge(v.str("challenge_id"), "pc_setup", null, "2026-11-03T12:02:00Z", v.str("signing_prefix_b64u"), changed)
        val e = runCatching { checks.verify(ch, SigKind.PC_SETUP, v.str("user_id"), v.str("device_id")) }.exceptionOrNull()
        assertEquals(RejectReason.CONTEXT_HASH, (e as ChallengeRejected).reason)
    }
}
