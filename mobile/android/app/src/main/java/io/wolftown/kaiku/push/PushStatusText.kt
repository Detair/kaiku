package io.wolftown.kaiku.push

import androidx.annotation.StringRes
import io.wolftown.kaiku.R

/** Maps the push status to the Settings status line. */
object PushStatusText {

    @StringRes
    fun forStatus(status: PushStatus): Int = when (status) {
        PushStatus.Unknown -> R.string.push_status_checking
        is PushStatus.Active -> when (status.kind) {
            TransportKind.FCM -> R.string.push_status_fcm
            TransportKind.UNIFIED_PUSH -> R.string.push_status_unifiedpush
        }
        is PushStatus.Off -> when (status.reason) {
            PushOffReason.PERMISSION_DENIED -> R.string.push_status_permission_off
            PushOffReason.NO_TRANSPORT -> R.string.push_status_no_transport
            PushOffReason.NO_DISTRIBUTOR -> R.string.push_status_no_distributor
            PushOffReason.ERROR -> R.string.push_status_error
        }
    }
}
