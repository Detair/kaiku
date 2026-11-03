package io.wolftown.kaiku.testutil

import io.wolftown.kaiku.data.local.AesGcmValueCipher
import javax.crypto.spec.SecretKeySpec

/** AES-256-GCM with a fixed test key, standing in for the Keystore-held key on the JVM. */
fun testValueCipher(): AesGcmValueCipher = AesGcmValueCipher({ SecretKeySpec(ByteArray(32) { 7 }, "AES") })
