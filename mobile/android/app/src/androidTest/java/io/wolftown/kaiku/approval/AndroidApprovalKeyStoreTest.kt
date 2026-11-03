package io.wolftown.kaiku.approval

import android.os.Build
import android.security.keystore.KeyInfo
import android.security.keystore.KeyProperties
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.security.KeyFactory
import java.security.KeyStore
import java.security.PrivateKey
import java.security.cert.CertificateFactory
import java.security.cert.X509Certificate
import org.junit.After
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Manual, on an emulator or phone with a PIN (not in CI): API 30+ and API 26–29.
 * `./gradlew :app:connectedDebugAndroidTest --tests '*AndroidApprovalKeyStoreTest*'`
 */
@RunWith(AndroidJUnit4::class)
class AndroidApprovalKeyStoreTest {

    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private val store = AndroidApprovalKeyStore(context)
    private val challenge = ByteArray(32) { (0x40 + it).toByte() }

    @After fun tearDown() = store.delete()

    private fun decision(): UvDecision.Use {
        val d = AndroidDeviceSuitabilityChecker(context).check()
        assumeTrue("needs a screen lock and a hardware keystore", d is UvDecision.Use)
        return d as UvDecision.Use
    }

    @Test fun generated_key_is_hardware_backed_and_needs_auth() {
        store.delete()
        store.generate(challenge, decision())
        val key = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
            .getKey(AndroidApprovalKeyStore.ALIAS, null) as PrivateKey
        val info = KeyFactory.getInstance(key.algorithm, "AndroidKeyStore").getKeySpec(key, KeyInfo::class.java)
        assertTrue(info.isUserAuthenticationRequired)
        assertFalse(info.isInvalidatedByBiometricEnrollment)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            assertTrue(
                info.securityLevel == KeyProperties.SECURITY_LEVEL_TRUSTED_ENVIRONMENT ||
                    info.securityLevel == KeyProperties.SECURITY_LEVEL_STRONGBOX,
            )
        } else {
            @Suppress("DEPRECATION")
            assertTrue(info.isInsideSecureHardware)
        }
    }

    @Test fun attestation_chain_carries_the_challenge() {
        store.delete()
        val generated = store.generate(challenge, decision())
        assertTrue(generated.attestationChain.isNotEmpty())
        val leaf = CertificateFactory.getInstance("X.509")
            .generateCertificate(generated.attestationChain.first().inputStream()) as X509Certificate
        val ext = leaf.getExtensionValue("1.3.6.1.4.1.11129.2.1.17")
        assertTrue("attestation extension present", ext != null)
        val hex = ext!!.joinToString("") { "%02x".format(it) }
        assertTrue("challenge in attestation", hex.contains(challenge.joinToString("") { "%02x".format(it) }))
    }

    @Test fun generate_never_replaces_an_existing_key() {
        store.delete()
        store.generate(challenge, decision())
        val first = store.publicKeySpki()!!
        val refused = runCatching { store.generate(challenge, decision()) }.isFailure
        assertTrue(refused)
        assertTrue(first.contentEquals(store.publicKeySpki()!!))
    }
}
