package io.wolftown.kaiku.push

import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import dagger.hilt.android.AndroidEntryPoint
import javax.inject.Inject

/**
 * Receives FCM data messages (C20 wake signals: `t`, `channel_id`, `count`,
 * all strings) and token rotations. It only parses and hands off: the fetch
 * runs in [WakeFetchWorker] via [WakeScheduler], the registration in
 * [PushRegistrar]. No network and no blocking work here. Never logs tokens.
 */
@AndroidEntryPoint
class KaikuFirebaseMessagingService : FirebaseMessagingService() {

    @Inject
    lateinit var registrar: PushRegistrar

    @Inject
    lateinit var wakeScheduler: WakeScheduler

    override fun onNewToken(token: String) {
        registrar.onNewFcmToken(token)
    }

    override fun onMessageReceived(message: RemoteMessage) {
        WakeSignalParser.fromFcmData(message.data)?.let(wakeScheduler::onWake)
    }
}
