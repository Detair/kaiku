package io.wolftown.kaiku.approval

import androidx.fragment.app.FragmentActivity
import java.security.KeyPair
import java.security.KeyPairGenerator
import java.security.Signature
import java.security.spec.ECGenParameterSpec

/** Software P-256 key store for JVM tests (the real one lives in the Android Keystore). */
class SoftwareApprovalKeyStore : ApprovalKeyStore {
    private var pair: KeyPair? = null
    var uvClass: UvClass? = null
    var invalidated = false
    var generatedChallenges = mutableListOf<ByteArray>()
    var onGenerate: (() -> Unit)? = null

    override fun hasKey(): Boolean = pair != null

    override fun generate(attestationChallenge: ByteArray, d: UvDecision.Use): GeneratedKey {
        check(pair == null) { "key exists" }
        onGenerate?.invoke()
        generatedChallenges += attestationChallenge
        pair = KeyPairGenerator.getInstance("EC").apply { initialize(ECGenParameterSpec("secp256r1")) }.generateKeyPair()
        uvClass = d.uvClass
        return GeneratedKey(pair!!.public.encoded, listOf(byteArrayOf(1, 2, 3), byteArrayOf(4, 5)), d.uvClass)
    }

    override fun publicKeySpki(): ByteArray? = pair?.public?.encoded

    override fun keyUvClass(): UvClass? = uvClass

    override fun delete() {
        pair = null
        uvClass = null
        invalidated = false
    }

    override fun signature(): Signature {
        if (invalidated) throw KeyInvalidatedException()
        val p = pair ?: throw NoApprovalKeyException()
        return Signature.getInstance("SHA256withECDSA").apply { initSign(p.private) }
    }
}

/** Records prompts; "verifies" at once and signs through the production [signNow] path, or returns [result]. */
class FakeUserVerifier(
    private val keyStore: ApprovalKeyStore,
    private val result: UvResult? = null,
) : UserVerifier {
    val prompts = mutableListOf<UvPrompt>()
    val signedMessages = mutableListOf<ByteArray>()

    override suspend fun sign(activity: FragmentActivity, prompt: UvPrompt, message: ByteArray): UvResult {
        prompts += prompt
        result?.let { return it }
        signedMessages += message
        return signNow(keyStore, message)
    }
}
