package io.wolftown.kaiku.data.voice

import org.webrtc.EglBase
import javax.inject.Inject
import javax.inject.Singleton

/**
 * The one process-wide [EglBase]. The video decoder of the
 * [PeerConnectionFactory][org.webrtc.PeerConnectionFactory] and every
 * `SurfaceViewRenderer` must share this EGL context, so every caller gets the
 * same instance; it is created on first use and never released.
 *
 * Production code MUST inject this rather than calling [EglBase.create] directly.
 * In tests, provide a `mockk<EglBaseProvider>(relaxed = true)` via the
 * [WebRtcManager] constructor; `get()` is only read if the test actually
 * touches `webRtcManager.eglBase`.
 */
interface EglBaseProvider {
    fun get(): EglBase
}

/** Creates its value on the first [get] and returns that same instance from then on. */
internal class SharedInstance<T : Any>(private val create: () -> T) {
    private val value: T by lazy(LazyThreadSafetyMode.SYNCHRONIZED) { create() }

    fun get(): T = value
}

@Singleton
class DefaultEglBaseProvider internal constructor(
    private val shared: SharedInstance<EglBase>,
) : EglBaseProvider {

    @Inject
    constructor() : this(SharedInstance { EglBase.create() })

    override fun get(): EglBase = shared.get()
}
