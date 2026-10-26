package io.wolftown.kaiku.push

import io.wolftown.kaiku.data.api.PushConfig

/** Everything the transport choice depends on; pure data so it can be unit tested. */
data class PushEnvironment(
    /** Google Play services are installed and usable. */
    val gmsAvailable: Boolean,
    /** Firebase project baked into the APK; null when built without `google-services.json`. */
    val firebaseProjectId: String?,
    /** What the server offers (C18). */
    val server: PushConfig,
    /** Installed UnifiedPush distributors (package names). */
    val distributors: List<String>,
    /** The user prefers UnifiedPush over FCM. */
    val preferUnifiedPush: Boolean,
    /** The distributor the user picked before, if any. */
    val savedDistributor: String?,
)

enum class PushOffReason { NO_TRANSPORT, NO_DISTRIBUTOR, PERMISSION_DENIED, ERROR }

sealed interface TransportChoice {
    data object Fcm : TransportChoice
    data class UnifiedPush(val distributor: String) : TransportChoice
    data class Off(val reason: PushOffReason) : TransportChoice
}

/** What the app reports about push delivery on this device. */
sealed interface PushStatus {
    data object Unknown : PushStatus
    data class Active(val kind: TransportKind, val distributor: String?) : PushStatus
    data class Off(val reason: PushOffReason) : PushStatus
}

/** Picks the push transport for this device. */
object PushTransportSelector {

    fun select(env: PushEnvironment): TransportChoice {
        val providers = env.server.providers
        val serverProject = env.server.fcm?.projectId
        // FCM only reaches this APK when server and APK use the same Firebase project.
        val fcmUsable = env.gmsAvailable &&
            !env.firebaseProjectId.isNullOrBlank() &&
            TransportKind.FCM.wire in providers &&
            serverProject == env.firebaseProjectId
        val upUsable = env.distributors.isNotEmpty() && TransportKind.UNIFIED_PUSH.wire in providers

        return when {
            env.preferUnifiedPush && upUsable -> unifiedPush(env)
            fcmUsable -> TransportChoice.Fcm
            upUsable -> unifiedPush(env)
            else -> TransportChoice.Off(PushOffReason.NO_TRANSPORT)
        }
    }

    /** The saved distributor while it is still installed, else the first one. */
    private fun unifiedPush(env: PushEnvironment): TransportChoice.UnifiedPush =
        TransportChoice.UnifiedPush(
            env.savedDistributor?.takeIf { it in env.distributors } ?: env.distributors.first()
        )
}
