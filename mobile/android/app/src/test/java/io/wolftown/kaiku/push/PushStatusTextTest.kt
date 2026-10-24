package io.wolftown.kaiku.push

import io.wolftown.kaiku.R
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Test

class PushStatusTextTest {

    private val all = listOf(
        PushStatus.Active(TransportKind.FCM, null) to R.string.push_status_fcm,
        PushStatus.Active(TransportKind.UNIFIED_PUSH, "io.heckel.ntfy") to R.string.push_status_unifiedpush,
        PushStatus.Off(PushOffReason.PERMISSION_DENIED) to R.string.push_status_permission_off,
        PushStatus.Off(PushOffReason.NO_TRANSPORT) to R.string.push_status_no_transport,
        PushStatus.Off(PushOffReason.ERROR) to R.string.push_status_error,
        PushStatus.Unknown to R.string.push_status_checking,
    )

    @Test
    fun `status text covers every status`() {
        all.forEach { (status, expected) ->
            val res = PushStatusText.forStatus(status)
            assertNotEquals(0, res)
            assertEquals(expected, res)
        }
        assertEquals(all.size, all.map { PushStatusText.forStatus(it.first) }.toSet().size)
    }

    @Test
    fun `unifiedpush text does not depend on the distributor`() {
        assertEquals(
            PushStatusText.forStatus(PushStatus.Active(TransportKind.UNIFIED_PUSH, "a")),
            PushStatusText.forStatus(PushStatus.Active(TransportKind.UNIFIED_PUSH, null)),
        )
    }
}
