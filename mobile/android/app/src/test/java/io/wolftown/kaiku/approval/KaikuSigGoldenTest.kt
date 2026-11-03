package io.wolftown.kaiku.approval

import io.wolftown.kaiku.testutil.SigFixtures.DEVICE_ID
import io.wolftown.kaiku.testutil.SigFixtures.EXPIRES_AT
import io.wolftown.kaiku.testutil.SigFixtures.NONCE
import io.wolftown.kaiku.testutil.SigFixtures.ORIGIN
import io.wolftown.kaiku.testutil.SigFixtures.PUBLIC_KEY
import io.wolftown.kaiku.testutil.SigFixtures.USER_ID
import io.wolftown.kaiku.testutil.SigFixtures.V01_CONTEXT
import io.wolftown.kaiku.testutil.SigFixtures.V02_CONTEXT
import io.wolftown.kaiku.testutil.SigFixtures.V03_CONTEXT
import io.wolftown.kaiku.testutil.SigFixtures.V04_CONTEXT
import io.wolftown.kaiku.testutil.SigFixtures.V05_CONTEXT
import io.wolftown.kaiku.testutil.SigFixtures.bytes
import io.wolftown.kaiku.testutil.SigFixtures.challengeId
import io.wolftown.kaiku.testutil.SigFixtures.hex
import io.wolftown.kaiku.testutil.SigFixtures.json
import io.wolftown.kaiku.testutil.SigFixtures.prefixFor
import io.wolftown.kaiku.testutil.SigFixtures.sha256
import io.wolftown.kaiku.testutil.SigFixtures.unhex
import io.wolftown.kaiku.testutil.SigFixtures.verifyP256
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The normative C26 golden vectors, reproduced byte for byte (canonical C,
 * context_hash, M or SHA-256(M) and length) and their signatures verified
 * with the public test key (V01, V02 and V04 have a high s).
 */
class KaikuSigGoldenTest {

    private val v01C = "4444444444444444844444444444444413d31e961a1ad8ec2f16b10c4c982e0876a878ad6df144566ee1894acb70f9c3" +
        "0102000000184b61696b75206465736b746f70206f6e2057696e646f7773000000006ae9ccc00000000f46616d696c69" +
        "65204dc3bc6c6c65720000000b48616d627572672c20444500000005616c696365"
    private val v01M = "6b61696b752d7369672d76310133333333333343338333333333333301000102030405060708090a0b0c0d0e0f101112" +
        "131415161718191a1b1c1d1e1f000000006ae9cd380000001868747470733a2f2f636861742e6578616d706c652e636f" +
        "6d11111111111141118111111111111111222222222222422282222222222222222e0d6af64a671a1cc0a3cf4435231f" +
        "fe08b56f6c3dbe9b74791da593e017fadf0100000020a0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9" +
        "babbbcbdbebf"

    @Test fun v01_login_qr() {
        val ctx = SigContext.fromJson(SigKind.LOGIN, json(V01_CONTEXT))
        assertEquals(v01C, ctx.canonical().hex())
        assertEquals("2e0d6af64a671a1cc0a3cf4435231ffe08b56f6c3dbe9b74791da593e017fadf", KaikuSig.contextHash(ctx).hex())
        val m = KaikuSig.message(prefixFor(SigKind.LOGIN, challengeId(1), ctx), Decision.APPROVE, bytes(0xa0..0xbf))
        assertEquals(198, m.size)
        assertEquals(v01M, m.hex())
        assertTrue(verifyP256(PUBLIC_KEY, m, "MEUCIDrPmE2yrciCLF2DnvgaeI1DzcPWXyyyPlD46qZ8WGQ8AiEAlpz219hjp91A-xULRpmddZWXEwsijLT5tAREO3WzLe0"))
    }

    @Test fun v02_enroll_admin_typed_code() {
        val ctx = SigContext.fromJson(SigKind.ENROLL, json(V02_CONTEXT))
        assertEquals(
            "5555555555554555855555555555555566666666666646668666666666666666000000036f6d61020106ce47f2ac3cd0" +
                "13e3ecada739938e70c625399b3a2a1387b75c480ca1ccffd400000007506978656c203802",
            ctx.canonical().hex(),
        )
        assertEquals("95cc384c6457731aa857a59bceeb1709add698e15c1cdce489bc8c2eeaed3c1b", KaikuSig.contextHash(ctx).hex())
        val m = KaikuSig.message(prefixFor(SigKind.ENROLL, challengeId(2), ctx), Decision.APPROVE, "123456".toByteArray(Charsets.US_ASCII))
        assertEquals(172, m.size)
        assertEquals("754b25e37dfac6fb063a3cc1c5b6238065ae171f0aae1da41aee6c3de74bd55c", sha256(m).hex())
        assertTrue(verifyP256(PUBLIC_KEY, m, "MEUCIE4YiBVDbJKd0nzIeslFWb5lP4OWx3fF5__aB5oaCk0HAiEAy8QaPa9Zeh43i9i9wVVDEKfPMsdM-ELJ2Y22NEF96pU"))
    }

    @Test fun v03_device_login() {
        val ctx = SigContext.fromJson(SigKind.DEVICE_LOGIN, json(V03_CONTEXT))
        assertEquals("03", ctx.canonical().hex())
        assertEquals("7bc190cbec9880136fdb564f293739d948deb11aeeb8dcaf95aa92c0424e29ae", KaikuSig.contextHash(ctx).hex())
        val m = KaikuSig.message(prefixFor(SigKind.DEVICE_LOGIN, challengeId(3), ctx), Decision.APPROVE, ByteArray(0))
        assertEquals(166, m.size)
        assertEquals("a5cbee81abacb5b317edacf06d75159bf80333278d65636c2f784ed8f0592703", sha256(m).hex())
        assertTrue(verifyP256(PUBLIC_KEY, m, "MEUCIQCfAESPWEcAVeTeo_Q3BDjOPw2yx_3eadQyYLl-b4MSZQIgIU0y3dtcSXviAGAepmzXTihWxL7LZHTuUZNH2lqMHp8"))
    }

    @Test fun v04_device_stepup() {
        val ctx = SigContext.fromJson(SigKind.DEVICE_STEPUP, json(V04_CONTEXT))
        // target_hash is computed on the device from the JSON target.
        assertEquals(
            "77777777777747778777777777777777000000116b65795f6465766963652e72656d6f7665249895243dc67001ed138a" +
                "d73a3cf6f8f60798146055289b12bbf12c0db7ceaf00000007506978656c2038",
            ctx.canonical().hex(),
        )
        assertArrayEquals(
            sha256(KaikuSig.lp("99999999-9999-4999-8999-999999999999".toByteArray())),
            unhex("249895243dc67001ed138ad73a3cf6f8f60798146055289b12bbf12c0db7ceaf"),
        )
        assertEquals("daa72edfe0362bf796d8c7ee30267354b9491e0ec97e36087db40e8d362f61c9", KaikuSig.contextHash(ctx).hex())
        val m = KaikuSig.message(prefixFor(SigKind.DEVICE_STEPUP, challengeId(4), ctx), Decision.APPROVE, ByteArray(0))
        assertEquals(166, m.size)
        assertEquals("f76b957ca40e9625cc10ee1cb54d6e38d50380f1af778b9e24efa6a94ae0ebf7", sha256(m).hex())
        assertTrue(verifyP256(PUBLIC_KEY, m, "MEYCIQDiXOgsy-XH9ewnDUbhcwC9s_NVOqI2QMCXDlGVgwWxzAIhAN5ZtWtqQTFZ9KTwykkn6uoRkWJwFyt7Ww8bCMWZXT0P"))
    }

    @Test fun v05_enroll_pop() {
        val ctx = SigContext.fromJson(SigKind.ENROLL_POP, json(V05_CONTEXT))
        assertEquals(
            "5555555555554555855555555555555548d5d9dd65d94e4d608344c215dd4fea099dccfd51c1aa3d86ca9783e4ac5f5d" +
                "404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f000000036f6d61",
            ctx.canonical().hex(),
        )
        assertEquals("0849d9cc3c467e1e5652673387a8847ba2a2803531cd9734256c5f58758122fc", KaikuSig.contextHash(ctx).hex())
        // new_key_hash = SHA-256(public_key DER)
        assertEquals(
            "48d5d9dd65d94e4d608344c215dd4fea099dccfd51c1aa3d86ca9783e4ac5f5d",
            sha256(io.wolftown.kaiku.testutil.SigFixtures.unb64u(PUBLIC_KEY)).hex(),
        )
        val prefix = prefixFor(
            SigKind.ENROLL_POP, challengeId(5), ctx,
            userId = "66666666-6666-4666-8666-666666666666",
            deviceId = "88888888-8888-4888-8888-888888888888",
        )
        val m = KaikuSig.message(prefix, Decision.APPROVE, ByteArray(0))
        assertEquals(166, m.size)
        assertEquals("7afd5d3b92e1a6f0f498df0cb963837c2f3f4231b55d590baf640921e85f8ad4", sha256(m).hex())
        assertTrue(verifyP256(PUBLIC_KEY, m, "MEUCIQDUWAn2yLiDOjJD_wRjiR_Y-SJQESYfWARIuGnkEVzPZAIgJ4dKWYPvbvjKfpgkCU0H_byn4nO4hz3_ZTQYmJALFzY"))
    }

    @Test fun prefix_parse_round_trip() {
        val ctx = SigContext.fromJson(SigKind.LOGIN, json(V01_CONTEXT))
        val p = KaikuSig.parsePrefix(prefixFor(SigKind.LOGIN, challengeId(1), ctx))
        assertEquals(SigKind.LOGIN.byte, p.kind)
        assertEquals(challengeId(1), p.challengeId)
        assertArrayEquals(NONCE, p.nonce)
        assertEquals(EXPIRES_AT, p.expiresAt)
        assertEquals(ORIGIN, p.origin)
        assertEquals(USER_ID, p.userId)
        assertEquals(DEVICE_ID, p.deviceId)
        assertArrayEquals(KaikuSig.contextHash(ctx), p.contextHash)
    }

    @Test fun prefix_with_trailing_or_missing_bytes_is_refused() {
        val ctx = SigContext.fromJson(SigKind.DEVICE_LOGIN, json(V03_CONTEXT))
        val p = prefixFor(SigKind.DEVICE_LOGIN, challengeId(3), ctx)
        assertThrows(SigFormatException::class.java) { KaikuSig.parsePrefix(p + byteArrayOf(0)) }
        assertThrows(SigFormatException::class.java) { KaikuSig.parsePrefix(p.copyOf(p.size - 1)) }
    }

    @Test fun lp_is_u32_big_endian() {
        assertEquals("00000003616263", KaikuSig.lp("abc".toByteArray()).hex())
        assertEquals("00000000", KaikuSig.lp(ByteArray(0)).hex())
        assertEquals("00000100", KaikuSig.lp(ByteArray(256)).copyOf(4).hex())
    }

    @Test fun uuid_bytes_follow_rfc9562() {
        assertEquals(
            "0123456789ab4def8123456789abcdef",
            KaikuSig.uuidBytes("01234567-89ab-4def-8123-456789abcdef").hex(),
        )
        assertThrows(SigFormatException::class.java) { KaikuSig.uuidBytes("1-2-3-4-5") }
        assertThrows(SigFormatException::class.java) { KaikuSig.uuidBytes("01234567-89AB-4DEF-8123-456789ABCDEF") }
    }

    @Test fun unknown_context_values_are_refused() {
        assertThrows(SigFormatException::class.java) {
            SigContext.fromJson(SigKind.LOGIN, json(V01_CONTEXT.replace("\"qr\"", "\"carrier_pigeon\"")))
        }
        assertThrows(SigFormatException::class.java) {
            SigContext.fromJson(SigKind.DEVICE_LOGIN, json("""{"client_class":42}"""))
        }
        assertThrows(SigFormatException::class.java) {
            SigContext.fromJson(SigKind.LOGIN, json(V01_CONTEXT.replace(""","username":"alice"""", "")))
        }
    }
}
