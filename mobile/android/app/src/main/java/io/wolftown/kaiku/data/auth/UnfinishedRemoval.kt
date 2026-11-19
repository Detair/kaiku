package io.wolftown.kaiku.data.auth

import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import java.util.concurrent.atomic.AtomicBoolean
import javax.inject.Inject
import javax.inject.Singleton

/**
 * "Use another account" on a signed-out key phone signs in with the key only to remove the phone.
 * If the app is killed while the removal's own confirmation is open, that session must not come
 * back as an ordinary sign-in the person never asked for: a mark is stored before the sign-in
 * ([KeyDeviceRepository.markRemovalSignIn]), and the next app start that finds it signs out the
 * plain way (the key stays; the phone shows "Confirm it's you") and clears it.
 */
@Singleton
class UnfinishedRemoval @Inject constructor(
    private val keyDevices: KeyDeviceRepository,
    private val signOut: KeyDeviceSignOut,
) {
    private val checked = AtomicBoolean(false)

    /**
     * At most once per process, at its first app start (a recreated screen of the same process
     * must not end a removal that is still running): whether the last run died during one.
     */
    fun takeAtStart(): Boolean = checked.compareAndSet(false, true) && keyDevices.removalSignInPending()

    /** The same sign-out as "Sign out" (key and push kept), then the mark goes. */
    suspend fun end() {
        signOut.signOut()
        keyDevices.markRemovalSignIn(false)
    }
}
