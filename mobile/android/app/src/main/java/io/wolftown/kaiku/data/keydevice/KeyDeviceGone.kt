package io.wolftown.kaiku.data.keydevice

import io.wolftown.kaiku.data.api.ApiException

/**
 * The server says this phone's key device is gone, in exactly one of the two answers its contract
 * defines for that: 404 `KEY_DEVICE_UNKNOWN` or 403 `KEY_DEVICE_REVOKED`. Only these make the app
 * forget its key; the same code with any other status (a proxy, a cache, a server fault) keeps it.
 */
fun ApiException.saysKeyDeviceGone(): Boolean = when (normalizedCode) {
    "KEY_DEVICE_UNKNOWN" -> status.value == 404
    "KEY_DEVICE_REVOKED" -> status.value == 403
    else -> false
}
