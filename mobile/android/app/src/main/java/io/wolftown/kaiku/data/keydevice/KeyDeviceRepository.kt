package io.wolftown.kaiku.data.keydevice

import io.wolftown.kaiku.approval.ApprovalKeyStore
import io.wolftown.kaiku.approval.GeneratedKey
import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.approval.UvDecision
import io.wolftown.kaiku.approval.keyId
import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.di.AuthCoroutineScope
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import java.time.Instant
import java.util.logging.Logger
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

    /**
     * No sign-in other than with this phone's key may complete now: a setup of this phone is
     * stored and not finished, or the key storage cannot be read ([appEntry]).
     */
    fun refusesOtherSignIn(): Boolean = false

    companion object {
        val NONE = KeyDevicePolicy { false }
    }
}

/**
 * Whether what the phone saved about its key can be read now. While it cannot, nothing reads,
 * refreshes, clears or recovers the stored session (the start, the background work, push): a
 * damaged or locked store is never taken as "no session" or "no key".
 */
fun interface KeyStorageCheck {
    fun unreadable(): Boolean

    companion object {
        val READABLE = KeyStorageCheck { false }
    }
}

/** What a change of the stored record did. Only [Done] changed anything. */
sealed interface Change {
    /** Written ([record]) or forgotten (null). */
    data class Done(val record: StoredKeyDevice?) : Change
    /** The stored record is not the one the caller expected: nothing changed. */
    data object Mismatch : Change
    /** The store cannot be read: nothing changed. */
    data object Unreadable : Change
    /** The write did not reach storage: nothing changed, here or in memory. */
    data object WriteFailed : Change

    val done: Boolean get() = this is Done
}

/** Why a record and its key are forgotten. Only these three exist (see [KeyDeviceRepository.forget]). */
enum class Forget {
    /** A server answer, decided by [KeyAnswers.settle]. */
    SERVER_ANSWER,
    /** Android invalidated this key (screen lock removed), found by a signature with it. */
    KEY_INVALIDATED,
    /** The person discarded a setup that can never finish and whose server stayed silent for long ([SetupDiscard]). */
    DISCARDED_BY_PERSON,
}

/** What [KeyDeviceRepository.generateKey] did. */
sealed interface NewKey {
    class Made(val key: GeneratedKey) : NewKey
    /** A record exists: no key is made beside it. */
    data object RecordExists : NewKey
    data object Unreadable : NewKey
}

/**
 * The key device outlives its sessions (B.4, SM4): the record in [store], the key in [keyStore].
 * This class is the only way to either of them, and every change names what it expects:
 *
 * - a record is written only where none is stored and the alias holds its key ([create]), or over
 *   the very record the caller holds ([replace], [update]);
 * - a record is forgotten only if it is the very record the caller holds ([forget]); its key goes
 *   with it only if the alias holds that record's key;
 * - a key is made only while no record is stored ([generateKey]); a key without a record is an
 *   unclaimed left-over and is deleted by key id only ([deleteUnclaimedKey]);
 * - while the store cannot be read ([unreadable]) nothing is written, forgotten, made or deleted,
 *   and a durable write that fails changes nothing in memory either.
 *
 * "The very record" is [StoredKeyDevice.sameAs]. All changes run under one monitor.
 * [state] is derived from the stored phase and the session, so a lapse (a refresh that ended the
 * session, or a plain sign-out) shows as [KeyDeviceState.Lapsed] the moment the session ends, with
 * the key, the user id and the push subscription kept.
 */
@Singleton
class KeyDeviceRepository @Inject constructor(
    private val store: KeyDeviceStore,
    private val keyStore: ApprovalKeyStore,
    private val authState: AuthState,
    @AuthCoroutineScope scope: CoroutineScope,
) : KeyDevicePolicy {

    private companion object {
        val logger: Logger = Logger.getLogger("KeyDeviceRepository")
    }

    /** What the store holds, as last read or written here. Set only through [set]. */
    @Volatile
    private var storage: KeyStorage = read()
    private val _record = MutableStateFlow((storage as? KeyStorage.Present)?.record)
    private val _unreadable = MutableStateFlow(storage is KeyStorage.Unreadable)

    /**
     * The stored record as it changes (null: none, or not readable). Its value is the record at
     * this moment (set together with every change); unlike [state] it needs no session.
     */
    val record: StateFlow<StoredKeyDevice?> = _record.asStateFlow()

    private fun set(s: KeyStorage) {
        storage = s
        _record.value = (s as? KeyStorage.Present)?.record
        _unreadable.value = s is KeyStorage.Unreadable
    }

    private fun stored(): StoredKeyDevice? = _record.value

    /** The store could not be read: nothing is known about a key device, and nothing is changed. */
    fun unreadable(): Boolean = storage is KeyStorage.Unreadable

    val state: StateFlow<KeyDeviceState> = combine(_record, authState.session) { s, session -> derive(s, session) }
        .stateIn(scope, SharingStarted.Eagerly, derive(stored(), authState.session.value))

    /** The state now, computed synchronously (navigation decisions use it). */
    fun current(): KeyDeviceState = derive(stored(), authState.session.value)

    /**
     * Where the running app belongs ([appEntry]), as it changes. The server address is a question
     * of the start only: here a phone without a setup or key belongs to the sign-in.
     */
    val entry: StateFlow<AppEntry> = combine(_unreadable, _record, authState.session) { u, r, s -> appEntry(u, r, s, hasServer = true) }
        .stateIn(scope, SharingStarted.Eagerly, appEntry(unreadable(), stored(), authState.session.value, hasServer = true))

    /** Where the app opens now ([appEntry]); [hasServer]: a server address is saved or built in. */
    fun entry(hasServer: Boolean): AppEntry = appEntry(unreadable(), stored(), authState.session.value, hasServer)

    override fun refusesOtherSignIn(): Boolean =
        appEntry(unreadable(), stored(), authState.session.value, hasServer = true).let { it == AppEntry.WAITING_SETUP || it == AppEntry.UNREADABLE }

    private fun derive(s: StoredKeyDevice?, session: AuthSession): KeyDeviceState = when {
        s == null -> KeyDeviceState.None
        s.phase == KeyDevicePhase.CLAIMING || s.phase == KeyDevicePhase.CLAIMED -> KeyDeviceState.None
        s.phase == KeyDevicePhase.PENDING ->
            s.readyAt?.let { KeyDeviceState.Pending(it, s.device.enrollmentId) } ?: KeyDeviceState.None
        session is AuthSession.LoggedIn && session.userId == s.device.userId -> KeyDeviceState.Active(s.device.userId)
        else -> KeyDeviceState.Lapsed(s.device.userId)
    }

    fun local(): LocalKeyDevice? = stored()?.device

    fun phase(): KeyDevicePhase? = stored()?.phase

    /** The origin a key ties this app to (B.1 origin pinning); null without a key device. */
    fun pinnedOrigin(): String? = stored()?.device?.origin

    fun sessionCap(): Instant? = stored()?.sessionCap

    fun lastRenewalPromptAt(): Instant? = stored()?.lastRenewalPromptAt

    override fun keepsPushOnSignOut(): Boolean = stored()?.phase == KeyDevicePhase.ACTIVE

    /**
     * Reads the store. A record written before records named their key takes the key in the alias
     * (there was always one alias and one record); it is not written back here.
     */
    private fun read(): KeyStorage = when (val s = store.load()) {
        is KeyStorage.Present -> if (s.record.keyId != null) s else try {
            KeyStorage.Present(s.record.copy(keyId = keyStore.keyId()))
        } catch (e: Exception) {
            logger.warning("Key not readable: ${e.javaClass.simpleName}")
            KeyStorage.Unreadable
        }
        else -> s
    }

    /** Reads the store again (after it could not be read); true when it can be read now. */
    @Synchronized
    fun reload(): Boolean {
        set(read())
        return storage !is KeyStorage.Unreadable
    }

    /**
     * Makes the key for a new setup. Only while the store is readable and holds no record; a key
     * the alias still holds then belongs to no record and is deleted first. Takes seconds: never
     * on the main thread.
     */
    @Synchronized
    fun generateKey(attestationChallenge: ByteArray, d: UvDecision.Use): NewKey {
        when (storage) {
            KeyStorage.Unreadable -> return NewKey.Unreadable
            is KeyStorage.Present -> return NewKey.RecordExists
            KeyStorage.Absent -> Unit
        }
        if (keyStore.hasKey()) {
            check(keyStore.delete(keyStore.keyId())) { "unclaimed key not deleted" }
        }
        return NewKey.Made(keyStore.generate(attestationChallenge, d))
    }

    /** Deletes a key no record names, only if it is [keyId]; true when it did. */
    @Synchronized
    fun deleteUnclaimedKey(keyId: String): Boolean {
        if (storage != KeyStorage.Absent) return false
        return try {
            keyStore.keyId() == keyId && keyStore.delete(keyId)
        } catch (e: Exception) {
            logger.warning("Unclaimed key not deleted: ${e.javaClass.simpleName}")
            false
        }
    }

    /** The first record of a setup: only where none is stored, and only for the key in the alias. */
    @Synchronized
    fun create(record: StoredKeyDevice): Change {
        when (storage) {
            KeyStorage.Unreadable -> return Change.Unreadable
            is KeyStorage.Present -> return Change.Mismatch
            KeyStorage.Absent -> Unit
        }
        val key = try {
            keyStore.keyId()
        } catch (e: Exception) {
            null
        }
        if (record.keyId == null || key != record.keyId) return Change.Mismatch
        return write(record)
    }

    /**
     * Puts [next] in place of [expected], only while [expected] is the stored record. A record
     * never changes its server, account, key or setup this way, and an install id it names stays
     * whatever [next] says.
     */
    @Synchronized
    fun replace(expected: StoredKeyDevice, next: StoredKeyDevice): Change {
        if (storage is KeyStorage.Unreadable) return Change.Unreadable
        val now = stored()
        if (now == null || !(now sameAs expected)) return Change.Mismatch
        require(
            next.device.origin == now.device.origin && next.device.userId == now.device.userId &&
                next.keyId == now.keyId && next.attemptId == now.attemptId,
        ) { "a record keeps its server, account, key and setup" }
        // What the stored record says about its install id stays.
        val kept = next.copy(installId = now.installId ?: next.installId, installIdLost = now.installIdLost || next.installIdLost)
        return write(kept)
    }

    /** [replace] with the stored record's current times kept: [change] gets the record as stored now. */
    @Synchronized
    fun update(expected: StoredKeyDevice, change: (StoredKeyDevice) -> StoredKeyDevice): Change {
        val now = stored() ?: return if (unreadable()) Change.Unreadable else Change.Mismatch
        if (!(now sameAs expected)) return Change.Mismatch
        return replace(now, change(now))
    }

    /**
     * A record written before records named their install id takes [id]: the id the app has kept,
     * under which such a record was enrolled. Null: the app has no id kept (it would have to make
     * a new one, which the server never heard of); the record then notes that its id is lost, for
     * good. A record that names one, or whose id is lost, keeps that, whatever [id] is.
     */
    @Synchronized
    fun bindInstallId(id: String?): Change {
        if (storage is KeyStorage.Unreadable) return Change.Unreadable
        val now = stored() ?: return Change.Mismatch
        if (now.installId != null || now.installIdLost) return Change.Done(now)
        return write(if (id != null) now.copy(installId = id) else now.copy(installIdLost = true))
    }

    /** The time of the last "stay signed in" prompt, kept beside [expected]. */
    fun markRenewalPrompted(expected: StoredKeyDevice, at: Instant): Change = update(expected) { it.copy(lastRenewalPromptAt = at) }

    /**
     * Forgets [expected] and its key, only while [expected] is the stored record. The record is
     * cleared durably first (a clear that fails deletes nothing); then the key goes if the alias
     * holds this record's key. Three callers exist: [KeyAnswers.settle] for a server answer,
     * [KeyInvalidation] for a key Android invalidated, and [SetupDiscard] for the person's own
     * decision about a setup that can never finish.
     */
    @Synchronized
    fun forget(expected: StoredKeyDevice, why: Forget): Change {
        if (storage is KeyStorage.Unreadable) return Change.Unreadable
        val now = stored()
        if (now == null || !(now sameAs expected)) return Change.Mismatch
        // A person's decision ends only a setup that can never finish: a claim that needs a code and
        // never got its answer. Never a record whose code was shown, a waiting or an active key.
        if (why == Forget.DISCARDED_BY_PERSON && !(now.phase == KeyDevicePhase.CLAIMING && now.codeClaim)) return Change.Mismatch
        // The decision was made on [expected]: it forgets only a record that is the same in
        // everything such a decision rests on, not merely the same record.
        if ((
                now.provisionalFrom != expected.provisionalFrom || now.notThisKey != expected.notThisKey ||
                    now.installIdLost != expected.installIdLost || now.installId != expected.installId
                )
        ) {
            return Change.Mismatch
        }
        if (!store.clear()) return Change.WriteFailed
        set(KeyStorage.Absent)
        logger.info("Key device forgotten: $why")
        try {
            // Another key in the alias is not this record's: it stays (no record names it, so the
            // next setup removes it as an unclaimed key).
            if (!keyStore.delete(now.keyId)) logger.warning("Alias holds another key than the forgotten record's")
        } catch (e: Exception) {
            logger.warning("Key not deleted: ${e.javaClass.simpleName}")
        }
        return Change.Done(null)
    }

    private fun write(s: StoredKeyDevice): Change {
        if (!store.save(s)) return Change.WriteFailed
        set(KeyStorage.Present(s))
        return Change.Done(s)
    }
}
