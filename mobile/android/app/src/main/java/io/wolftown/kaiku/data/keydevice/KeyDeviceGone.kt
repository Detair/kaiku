package io.wolftown.kaiku.data.keydevice

import io.wolftown.kaiku.data.api.ApiException

/**
 * The server says this phone's key device is gone, in exactly one of the two answers its contract
 * defines for that: 404 `KEY_DEVICE_UNKNOWN` or 403 `KEY_DEVICE_REVOKED` (the rows of
 * [KeyAnswers.rows]). The same code with any other status (a proxy, a cache, a server fault) is not.
 */
fun ApiException.saysKeyDeviceGone(): Boolean =
    KeyAnswers.verdict(Ask.DEVICE_LOGIN_CHALLENGE, ServerAnswer.of(this)) == Verdict.KEY_GONE
