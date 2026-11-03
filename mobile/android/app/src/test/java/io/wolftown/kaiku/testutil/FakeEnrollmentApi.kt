package io.wolftown.kaiku.testutil

import io.wolftown.kaiku.approval.SigContext
import io.wolftown.kaiku.approval.SigKind
import io.wolftown.kaiku.data.api.EnrollmentApi
import io.wolftown.kaiku.domain.model.AccountRef
import io.wolftown.kaiku.domain.model.ApprovalChallenge
import io.wolftown.kaiku.domain.model.ClaimResult
import io.wolftown.kaiku.domain.model.ClaimStatus
import io.wolftown.kaiku.domain.model.Enrollment
import io.wolftown.kaiku.domain.model.EnrollmentClaim
import io.wolftown.kaiku.domain.model.EnrollmentIssued
import io.wolftown.kaiku.domain.model.EnrollmentOpen
import io.wolftown.kaiku.domain.model.EnrollmentState
import io.wolftown.kaiku.domain.model.PopChallenge
import java.util.ArrayDeque

/** Scripted [EnrollmentApi]; the PoP challenge is built for the key the phone sends, as the server would. */
class FakeEnrollmentApi {
    val calls = mutableListOf<String>()
    val nonce: ByteArray = SigFixtures.bytes(0x40..0x5f)
    var subject = AccountRef(SUBJECT_ID, "oma", "Oma")
    var serverName = "Familie Müller"
    var claimFailure: Exception? = null
    var claimResult = ClaimResult("55555555-5555-4555-8555-555555555555", "claimed", code = "123456",
        codeExpiresAt = "2026-11-03T12:05:00Z", claimSecret = "claim-secret")
    var lastClaim: EnrollmentClaim? = null
    var lastTicket: String? = null
    val statuses = ArrayDeque<ClaimStatus>()
    val statusFailures = ArrayDeque<Exception>()
    var lastOrigin: String? = null
    var issued = EnrollmentIssued("55555555-5555-4555-8555-555555555555", "setup-token-from-ticket",
        "https://chat.example.com/setup#v=1&t=x", "2026-11-03T12:10:00Z")
    var enrollment: Enrollment? = null
    var getFailure: Exception? = null

    companion object {
        const val SUBJECT_ID = "66666666-6666-4666-8666-666666666666"
    }

    fun popChallengeFor(publicKeyB64u: String, userId: String = SUBJECT_ID): ApprovalChallenge {
        val json = """{"enrollment_id":"55555555-5555-4555-8555-555555555555","new_key_hash":"${SigFixtures.b64u(SigFixtures.sha256(SigFixtures.unb64u(publicKeyB64u)))}","attestation_challenge":"${SigFixtures.b64u(nonce)}","subject_username":"${subject.username}"}"""
        val ctx = SigContext.fromJson(SigKind.ENROLL_POP, SigFixtures.json(json))
        val prefix = SigFixtures.prefixFor(SigKind.ENROLL_POP, SigFixtures.challengeId(5), ctx, userId = userId)
        return SigFixtures.challengeFor(SigKind.ENROLL_POP, 5, json, prefix = prefix)
    }

    val api = object : EnrollmentApi {
        override suspend fun issueOwn(): EnrollmentIssued { calls += "issueOwn"; return issued }
        override suspend fun issueWithTicket(ticket: String): EnrollmentIssued { calls += "issueWithTicket"; lastTicket = ticket; return issued }
        override suspend fun get(id: String): Enrollment {
            calls += "get:$id"
            getFailure?.let { throw it }
            return enrollment ?: Enrollment(id, "own", EnrollmentState.ISSUED, subject)
        }
        override suspend fun cancel(id: String) { calls += "cancel:$id" }
        override suspend fun open(setupToken: String, origin: String?): EnrollmentOpen {
            calls += "open"; lastOrigin = origin
            return EnrollmentOpen(serverName, SigFixtures.b64u(nonce), "2026-11-03T12:10:00Z")
        }
        override suspend fun popChallenge(setupToken: String, deviceId: String, publicKeyB64u: String, origin: String?): PopChallenge {
            calls += "popChallenge"
            return PopChallenge(popChallengeFor(publicKeyB64u), subject)
        }
        override suspend fun claim(req: EnrollmentClaim, ticket: String?, origin: String?): ClaimResult {
            calls += "claim"; lastClaim = req; lastTicket = ticket
            claimFailure?.let { throw it }
            return claimResult
        }
        override suspend fun status(enrollmentId: String, claimSecret: String, waitSeconds: Int, origin: String?): ClaimStatus {
            calls += "status"
            statusFailures.pollFirst()?.let { throw it }
            // A long-poll with nothing new waits (until the test ends).
            return statuses.pollFirst() ?: kotlinx.coroutines.awaitCancellation()
        }
    }
}
