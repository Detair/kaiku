package io.wolftown.kaiku.data.keydevice

import io.wolftown.kaiku.data.local.AuthSession

/** Where the app has to open, and where a phone without a session belongs while it runs. */
enum class AppEntry {
    /** A session runs. */
    HOME,
    /** A key device without a session: it signs in again with its screen lock. */
    CONFIRM_ITS_YOU,
    /** A setup of this phone is stored and not finished: its waiting screen, nothing else. */
    WAITING_SETUP,
    /** The sign-in form. */
    SIGN_IN,
    /** No server is known: the server address comes first. */
    SERVER_ADDRESS,
    /** What the phone has saved about its key cannot be read: nothing is decided until it can. */
    UNREADABLE,
}

/**
 * The one answer to "where does this phone belong" ([unreadable]: the key storage cannot be read;
 * [record]: the stored key record; [session]: the session now; [hasServer]: a server address is
 * saved or built in). The start of the app, the navigation while signed out and the sign-in
 * itself ([setupOwnsEntry]) all go by it.
 *
 * A stored setup that has not finished (claim sent, code shown, or waiting for its ready time)
 * owns the entry: until it ends through the key storage, no sign-in form is shown and no other
 * sign-in completes.
 */
fun appEntry(unreadable: Boolean, record: StoredKeyDevice?, session: AuthSession, hasServer: Boolean): AppEntry = when {
    unreadable -> AppEntry.UNREADABLE
    session is AuthSession.LoggedIn -> AppEntry.HOME
    setupOwnsEntry(record) -> AppEntry.WAITING_SETUP
    record?.phase == KeyDevicePhase.ACTIVE -> AppEntry.CONFIRM_ITS_YOU
    !hasServer -> AppEntry.SERVER_ADDRESS
    else -> AppEntry.SIGN_IN
}

/** A stored setup that has not finished: claim sent, code shown, or waiting for its ready time. */
fun setupOwnsEntry(record: StoredKeyDevice?): Boolean =
    record?.phase == KeyDevicePhase.CLAIMING || record?.phase == KeyDevicePhase.CLAIMED || record?.phase == KeyDevicePhase.PENDING
