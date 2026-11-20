package io.wolftown.kaiku.ui.auth.setup

import io.wolftown.kaiku.data.auth.OriginPin
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import javax.inject.Inject
import javax.inject.Provider

/**
 * The one rule for whether the app's configured server address may change now. Both writers ask
 * it immediately before they write, with nothing in between: the server-address screen and a
 * setup adopting its link's server ([AndroidSetupEnvironment.adoptServer]).
 *
 * Not while what the phone has saved about its key cannot be read, a key record is stored (a key,
 * or a setup that has not finished), the app is tied to a server ([OriginPin]), or the setup of
 * this phone holds an attempt ([SetupOwner.holdsAttempt]).
 */
class ServerAddressRule internal constructor(
    private val keyDevices: KeyDeviceRepository,
    private val pinned: () -> String?,
    private val setupHolds: () -> Boolean,
) {
    @Inject
    constructor(keyDevices: KeyDeviceRepository, originPin: OriginPin, setupOwner: Provider<SetupOwner>) :
        this(keyDevices, originPin::pinned, { setupOwner.get().holdsAttempt() })

    /**
     * [forOwnSetup]: asked by the setup for the attempt it is running, which does not count
     * against itself (any other caller leaves it false).
     */
    fun mayChange(forOwnSetup: Boolean = false): Boolean =
        !keyDevices.unreadable() && keyDevices.record.value == null && pinned() == null && (forOwnSetup || !setupHolds())
}
