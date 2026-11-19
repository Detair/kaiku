package io.wolftown.kaiku.data.keydevice

import io.wolftown.kaiku.data.api.ApiErrorFields
import io.wolftown.kaiku.testutil.FakeApprovalApi
import io.wolftown.kaiku.testutil.SigFixtures
import io.wolftown.kaiku.testutil.apiError
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Test
import java.time.Instant

/** The probe reports a key as gone only in the two answers the server's contract defines for it. */
class KeyDeviceProbeTest {

    private val api = FakeApprovalApi()
    private val probe = KeyDeviceProbe(api) { SigFixtures.DEVICE_ID }

    private suspend fun answer(x: Exception): ServerKeyState {
        api.failWith = x
        return probe.probe(SigFixtures.USER_ID, SigFixtures.ORIGIN)
    }

    @Test fun `only 404 unknown and 403 revoked say the key is gone`() = runTest {
        assertEquals(ServerKeyState.Gone, answer(apiError(404, "KEY_DEVICE_UNKNOWN")))
        assertEquals(ServerKeyState.Gone, answer(apiError(403, "KEY_DEVICE_REVOKED")))
        assertEquals(ServerKeyState.Gone, answer(apiError(404, "key_device_unknown")))
    }

    @Test fun `the same codes with any other status keep the key`() = runTest {
        for (x in listOf(
            apiError(503, "KEY_DEVICE_UNKNOWN"),
            apiError(500, "KEY_DEVICE_REVOKED"),
            apiError(403, "KEY_DEVICE_UNKNOWN"),
            apiError(404, "KEY_DEVICE_REVOKED"),
            apiError(200, "KEY_DEVICE_UNKNOWN"),
            apiError(410, "KEY_DEVICE_REVOKED"),
            apiError(404, "NOT_FOUND"),
        )) {
            assertEquals(x.toString(), ServerKeyState.Unknown, answer(x))
        }
    }

    @Test fun `pending and offline are told apart`() = runTest {
        val at = Instant.parse("2026-11-20T08:00:00Z")
        assertEquals(ServerKeyState.Pending(at), answer(apiError(409, "KEY_DEVICE_PENDING", ApiErrorFields(readyAt = at.toString()))))
        assertEquals(ServerKeyState.Unknown, answer(java.io.IOException("offline")))
    }
}
