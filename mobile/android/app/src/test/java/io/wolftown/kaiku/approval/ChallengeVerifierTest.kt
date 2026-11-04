package io.wolftown.kaiku.approval

import io.wolftown.kaiku.testutil.SigFixtures.DEVICE_ID
import io.wolftown.kaiku.testutil.SigFixtures.EXPIRES_AT
import io.wolftown.kaiku.testutil.SigFixtures.ORIGIN
import io.wolftown.kaiku.testutil.SigFixtures.USER_ID
import io.wolftown.kaiku.testutil.SigFixtures.V01_CONTEXT
import io.wolftown.kaiku.testutil.SigFixtures.V03_CONTEXT
import io.wolftown.kaiku.testutil.SigFixtures.challengeFor
import io.wolftown.kaiku.testutil.SigFixtures.challengeId
import io.wolftown.kaiku.testutil.SigFixtures.json
import io.wolftown.kaiku.testutil.SigFixtures.prefixFor
import org.junit.Assert.assertEquals
import org.junit.Assert.fail
import org.junit.Test

class ChallengeVerifierTest {

    private var pinned: String? = ORIGIN
    private var now = EXPIRES_AT - 30
    private val verifier = ChallengeVerifier(pinnedOrigin = { pinned }, nowSeconds = { now })

    private val loginCtx = SigContext.fromJson(SigKind.LOGIN, json(V01_CONTEXT))

    private fun rejected(block: () -> Unit): RejectReason {
        try {
            block()
        } catch (e: ChallengeRejected) {
            return e.reason
        }
        fail("expected ChallengeRejected")
        error("unreachable")
    }

    @Test fun accepts_v01_with_pinned_origin() {
        val v = verifier.verify(challengeFor(SigKind.LOGIN, 1, V01_CONTEXT, "qr"), SigKind.LOGIN, USER_ID, DEVICE_ID)
        assertEquals(challengeId(1), v.challengeId)
        assertEquals(SigKind.LOGIN, v.kind)
        assertEquals(EXPIRES_AT, v.expiresAt)
        val ctx = v.context as LoginContext
        assertEquals("Kaiku desktop on Windows", ctx.clientLabel)
        assertEquals("alice", ctx.username)
        assertEquals("Familie Müller", ctx.serverName)
    }

    @Test fun other_origin_is_refused() {
        pinned = "https://evil.example.com"
        assertEquals(RejectReason.ORIGIN, rejected {
            verifier.verify(challengeFor(SigKind.LOGIN, 1, V01_CONTEXT), SigKind.LOGIN, USER_ID, DEVICE_ID)
        })
    }

    @Test fun no_pinned_origin_is_refused() {
        pinned = null
        assertEquals(RejectReason.ORIGIN, rejected {
            verifier.verify(challengeFor(SigKind.LOGIN, 1, V01_CONTEXT), SigKind.LOGIN, USER_ID, DEVICE_ID)
        })
    }

    @Test fun reserved_agent_kind_byte_is_unknown() {
        val p = prefixFor(SigKind.LOGIN, challengeId(1), loginCtx, kindByte = 0x10)
        assertEquals(RejectReason.UNKNOWN_KIND, rejected {
            verifier.verify(challengeFor(SigKind.LOGIN, 1, V01_CONTEXT, prefix = p), SigKind.LOGIN, USER_ID, DEVICE_ID)
        })
    }

    @Test fun kind_mismatch_with_the_screen_is_refused() {
        // A valid device_login challenge offered to the login approval screen.
        assertEquals(RejectReason.KIND_MISMATCH, rejected {
            verifier.verify(challengeFor(SigKind.DEVICE_LOGIN, 3, V03_CONTEXT), SigKind.LOGIN, USER_ID, DEVICE_ID)
        })
        // JSON kind says login but the signed byte says device_login.
        val p = prefixFor(SigKind.DEVICE_LOGIN, challengeId(3), SigContext.fromJson(SigKind.DEVICE_LOGIN, json(V03_CONTEXT)))
        assertEquals(RejectReason.KIND_MISMATCH, rejected {
            verifier.verify(
                challengeFor(SigKind.DEVICE_LOGIN, 3, V03_CONTEXT, prefix = p, wireKind = "login"),
                SigKind.DEVICE_LOGIN, USER_ID, DEVICE_ID,
            )
        })
    }

    @Test fun other_challenge_id_is_refused() {
        val p = prefixFor(SigKind.LOGIN, challengeId(9), loginCtx)
        assertEquals(RejectReason.CHALLENGE_ID, rejected {
            verifier.verify(challengeFor(SigKind.LOGIN, 1, V01_CONTEXT, prefix = p), SigKind.LOGIN, USER_ID, DEVICE_ID)
        })
    }

    @Test fun other_user_is_refused() {
        assertEquals(RejectReason.USER, rejected {
            verifier.verify(challengeFor(SigKind.LOGIN, 1, V01_CONTEXT), SigKind.LOGIN, "66666666-6666-4666-8666-666666666666", DEVICE_ID)
        })
    }

    @Test fun other_device_is_refused() {
        assertEquals(RejectReason.DEVICE, rejected {
            verifier.verify(challengeFor(SigKind.LOGIN, 1, V01_CONTEXT), SigKind.LOGIN, USER_ID, "88888888-8888-4888-8888-888888888888")
        })
    }

    @Test fun expired_beyond_sixty_seconds_is_refused() {
        now = EXPIRES_AT + 59
        verifier.verify(challengeFor(SigKind.LOGIN, 1, V01_CONTEXT), SigKind.LOGIN, USER_ID, DEVICE_ID)
        now = EXPIRES_AT + 60
        assertEquals(RejectReason.EXPIRED, rejected {
            verifier.verify(challengeFor(SigKind.LOGIN, 1, V01_CONTEXT), SigKind.LOGIN, USER_ID, DEVICE_ID)
        })
    }

    @Test fun changed_client_label_breaks_the_context_hash() {
        val honest = challengeFor(SigKind.LOGIN, 1, V01_CONTEXT)
        val shown = honest.copy(context = json(V01_CONTEXT.replace("Kaiku desktop on Windows", "Your own phone")))
        assertEquals(RejectReason.CONTEXT_HASH, rejected {
            verifier.verify(shown, SigKind.LOGIN, USER_ID, DEVICE_ID)
        })
    }

    @Test fun wrong_magic_is_refused() {
        val p = prefixFor(SigKind.LOGIN, challengeId(1), loginCtx, magic = "kaiku-sig-v2")
        assertEquals(RejectReason.MAGIC, rejected {
            verifier.verify(challengeFor(SigKind.LOGIN, 1, V01_CONTEXT, prefix = p), SigKind.LOGIN, USER_ID, DEVICE_ID)
        })
    }

    @Test fun broken_prefix_and_context_are_format_errors() {
        val ch = challengeFor(SigKind.LOGIN, 1, V01_CONTEXT)
        assertEquals(RejectReason.FORMAT, rejected {
            verifier.verify(ch.copy(signingPrefix = "!!"), SigKind.LOGIN, USER_ID, DEVICE_ID)
        })
        assertEquals(RejectReason.FORMAT, rejected {
            verifier.verify(ch.copy(context = json("""{"client_class":"android"}""")), SigKind.LOGIN, USER_ID, DEVICE_ID)
        })
    }
}
