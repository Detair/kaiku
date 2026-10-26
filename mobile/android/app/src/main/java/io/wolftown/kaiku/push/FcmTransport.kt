package io.wolftown.kaiku.push

import android.content.Context
import com.google.firebase.FirebaseApp
import com.google.firebase.messaging.FirebaseMessaging
import dagger.hilt.android.qualifiers.ApplicationContext
import kotlinx.coroutines.tasks.await
import javax.inject.Inject

/**
 * FCM: the registration token is the endpoint. Token generation is off in the
 * manifest (`firebase_messaging_auto_init_enabled=false`) and only switched
 * on while FCM is the chosen transport. Without a Firebase project in the APK
 * every call is a no-op (the selector never picks FCM then).
 */
class FcmTransport @Inject constructor(
    @ApplicationContext private val context: Context,
) : PushTransport {

    override val kind: TransportKind = TransportKind.FCM

    private fun messagingOrNull(): FirebaseMessaging? =
        if (FirebaseApp.getApps(context).isEmpty()) null else FirebaseMessaging.getInstance()

    override suspend fun requestEndpoint(onEndpoint: suspend (String) -> Unit) {
        val messaging = messagingOrNull()
            ?: throw IllegalStateException("Firebase is not configured in this build")
        messaging.isAutoInitEnabled = true
        val token = messaging.token.await()
        onEndpoint(token)
    }

    override suspend fun unregister() {
        val messaging = messagingOrNull() ?: return
        messaging.isAutoInitEnabled = false
        messaging.deleteToken().await()
    }
}
