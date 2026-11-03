package io.wolftown.kaiku.data.keydevice

import io.wolftown.kaiku.approval.ApprovalKeyStore
import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.di.AuthCoroutineScope
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import java.time.Instant
import javax.inject.Inject
import javax.inject.Singleton

/** L2-A13: this phone's key-device state (E1's start destination reads it). */
sealed interface KeyDeviceState {
    data object None : KeyDeviceState
    data class Pending(val readyAt: Instant, val enrollmentId: String) : KeyDeviceState
    data class Active(val userId: String) : KeyDeviceState
    data class Lapsed(val userId: String) : KeyDeviceState
}

/**
 * The local key device. [keyDeviceId] is the server's id (learned at the first
 * device-key sign-in from `GET /auth/me` `session.key_device_id`); [claimSecret]
 * polls the enrolment status; [accountName] is shown on "Confirm it's you".
 */
data class LocalKeyDevice(
    val keyDeviceId: String?,
    val userId: String,
    val origin: String,
    val uvClass: UvClass,
    val enrollmentId: String,
    val claimSecret: String?,
    val accountName: String? = null,
) {
    override fun toString(): String =
        "LocalKeyDevice(keyDeviceId=$keyDeviceId, userId=$userId, origin=$origin, uvClass=$uvClass, enrollmentId=$enrollmentId)"
}

/** Whether a plain sign-out keeps the push subscription (C19 amendment 3): only on a key device. */
fun interface KeyDevicePolicy {
    fun keepsPushOnSignOut(): Boolean

    companion object {
        val NONE = KeyDevicePolicy { false }
    }
}

/**
 * The key device outlives its sessions (B.4, SM4): metadata in [store], the key
 * in [keyStore]. [state] is derived from the stored phase and the session, so a
 * lapse (a refresh that ended the session, or a plain sign-out) shows as
 * [KeyDeviceState.Lapsed] the moment the session ends, with the key, the user
 * id and the push subscription kept.
 */
@Singleton
class KeyDeviceRepository @Inject constructor(
    private val store: KeyDeviceStore,
    private val keyStore: ApprovalKeyStore,
    private val authState: AuthState,
    @AuthCoroutineScope scope: CoroutineScope,
) : KeyDevicePolicy {

    private val stored = MutableStateFlow(store.load())

    val state: StateFlow<KeyDeviceState> = combine(stored, authState.session) { s, session -> derive(s, session) }
        .stateIn(scope, SharingStarted.Eagerly, derive(stored.value, authState.session.value))

    /** The state now, computed synchronously (navigation decisions use it). */
    fun current(): KeyDeviceState = derive(stored.value, authState.session.value)

    private fun derive(s: StoredKeyDevice?, session: AuthSession): KeyDeviceState = when {
        s == null -> KeyDeviceState.None
        s.phase == KeyDevicePhase.CLAIMED -> KeyDeviceState.None
        s.phase == KeyDevicePhase.PENDING ->
            s.readyAt?.let { KeyDeviceState.Pending(it, s.device.enrollmentId) } ?: KeyDeviceState.None
        session is AuthSession.LoggedIn && session.userId == s.device.userId -> KeyDeviceState.Active(s.device.userId)
        else -> KeyDeviceState.Lapsed(s.device.userId)
    }

    fun local(): LocalKeyDevice? = stored.value?.device

    fun phase(): KeyDevicePhase? = stored.value?.phase

    /** The origin a key ties this app to (B.1 origin pinning); null without a key device. */
    fun pinnedOrigin(): String? = stored.value?.device?.origin

    /** A setup claimed a code and waits for its confirmation (not a key yet). */
    fun saveClaimed(d: LocalKeyDevice) = write(StoredKeyDevice(d, KeyDevicePhase.CLAIMED))

    fun markPending(d: LocalKeyDevice, readyAt: Instant) = write(StoredKeyDevice(d, KeyDevicePhase.PENDING, readyAt))

    /** After a device-key sign-in of [userId]; the claim secret is no longer needed. */
    fun markActive(userId: String, keyDeviceId: String?) {
        val s = stored.value ?: return
        if (s.device.userId != userId) return
        write(
            s.copy(
                device = s.device.copy(keyDeviceId = keyDeviceId ?: s.device.keyDeviceId, claimSecret = null),
                phase = KeyDevicePhase.ACTIVE,
                readyAt = null,
            ),
        )
    }

    /** Kept for L2-A8/A13 callers: a lapse is derived from the ended session, so nothing is written. */
    fun markLapsed() = Unit

    /** Deletes the key and the metadata (revoked, invalidated, removed, cancelled setup). */
    fun forget() {
        keyStore.delete()
        store.clear()
        stored.value = null
    }

    fun recordSessionCap(cap: Instant?) {
        val s = stored.value ?: return
        write(s.copy(sessionCap = cap))
    }

    fun sessionCap(): Instant? = stored.value?.sessionCap

    fun lastRenewalPromptAt(): Instant? = stored.value?.lastRenewalPromptAt

    fun markRenewalPrompted(at: Instant) {
        val s = stored.value ?: return
        write(s.copy(lastRenewalPromptAt = at))
    }

    override fun keepsPushOnSignOut(): Boolean = stored.value?.phase == KeyDevicePhase.ACTIVE

    private fun write(s: StoredKeyDevice) {
        store.save(s)
        stored.value = s
    }
}
