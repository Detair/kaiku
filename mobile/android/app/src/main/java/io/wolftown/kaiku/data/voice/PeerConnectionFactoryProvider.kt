package io.wolftown.kaiku.data.voice

import android.content.Context
import dagger.hilt.android.qualifiers.ApplicationContext
import org.webrtc.PeerConnectionFactory
import org.webrtc.audio.JavaAudioDeviceModule
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

/**
 * The process-wide [PeerConnectionFactory]. One factory and one audio device
 * module per process, shared by every voice join; nothing disposes them.
 * [WebRtcManager.dispose] releases only peer connections, tracks and sources.
 */
interface PeerConnectionFactoryProvider {
    fun get(): PeerConnectionFactory
}

/**
 * Creates the factory on first [get] and returns the same instance after
 * that. [create] is replaceable so JVM tests can count creations without
 * native WebRTC.
 */
@Singleton
class DefaultPeerConnectionFactoryProvider internal constructor(
    private val create: () -> PeerConnectionFactory,
) : PeerConnectionFactoryProvider {

    @Inject
    constructor(@ApplicationContext context: Context) : this({ createFactory(context) })

    @Volatile
    private var factory: PeerConnectionFactory? = null

    override fun get(): PeerConnectionFactory =
        factory ?: synchronized(this) {
            factory ?: create().also { factory = it }
        }

    private companion object {
        private val logger = Logger.getLogger("PeerConnectionFactoryProvider")

        fun createFactory(context: Context): PeerConnectionFactory {
            PeerConnectionFactory.initialize(
                PeerConnectionFactory.InitializationOptions.builder(context)
                    .setEnableInternalTracer(false)
                    .createInitializationOptions()
            )
            // Lives as long as the factory, i.e. the process.
            val audioDeviceModule = JavaAudioDeviceModule.builder(context)
                .createAudioDeviceModule()
            return PeerConnectionFactory.builder()
                .setAudioDeviceModule(audioDeviceModule)
                .createPeerConnectionFactory()
                .also { logger.info("PeerConnectionFactory initialized") }
        }
    }
}
