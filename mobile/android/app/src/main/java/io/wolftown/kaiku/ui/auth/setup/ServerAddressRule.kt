package io.wolftown.kaiku.ui.auth.setup

import io.wolftown.kaiku.data.auth.OriginPin
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import javax.inject.Inject
import javax.inject.Provider

/**
 * The one rule for whether the app's configured server address may change now, asked by the
 * server-address screen immediately before it writes, with nothing in between. (A setup or a
 * registration never writes it: they talk to the origin they started with.)
 *
 * Not while what the phone has saved about its key cannot be read, a key record is stored (a key,
 * or a setup that has not finished), the app is tied to a server ([OriginPin]), the setup of this
 * phone holds an attempt ([SetupOwner.holdsAttempt]), or a registration is on its way
 * ([io.wolftown.kaiku.ui.auth.Registrations]).
 */
class ServerAddressRule internal constructor(
    private val keyDevices: KeyDeviceRepository,
    private val pinned: () -> String?,
    private val setupHolds: () -> Boolean,
    private val registering: () -> Boolean = { false },
) {
    @Inject
    constructor(
        keyDevices: KeyDeviceRepository,
        originPin: OriginPin,
        setupOwner: Provider<SetupOwner>,
        registrations: io.wolftown.kaiku.ui.auth.Registrations,
    ) : this(keyDevices, originPin::pinned, { setupOwner.get().holdsAttempt() }, registrations::running)

    fun mayChange(): Boolean =
        !keyDevices.unreadable() && keyDevices.record.value == null && pinned() == null && !setupHolds() && !registering()
}
