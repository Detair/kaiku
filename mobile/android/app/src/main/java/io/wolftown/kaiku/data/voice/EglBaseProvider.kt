package io.wolftown.kaiku.data.voice

import org.webrtc.EglBase
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Indirection over [EglBase.create] so tests can substitute the EGL stack
 * without invoking native EGL14 JNI.
 *
 * Production code MUST inject this rather than calling [EglBase.create] directly.
 * In tests, provide a `mockk<EglBaseProvider>(relaxed = true)` via the
 * [WebRtcManager] constructor; the `create()` return is only read if the test
 * actually touches `webRtcManager.eglBase`.
 */
interface EglBaseProvider {
    fun create(): EglBase
}

@Singleton
class DefaultEglBaseProvider @Inject constructor() : EglBaseProvider {
    override fun create(): EglBase = EglBase.create()
}
