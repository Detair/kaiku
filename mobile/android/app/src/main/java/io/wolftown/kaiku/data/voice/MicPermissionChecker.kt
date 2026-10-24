package io.wolftown.kaiku.data.voice

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import androidx.core.content.ContextCompat
import dagger.hilt.android.qualifiers.ApplicationContext
import javax.inject.Inject

/** Answers whether the microphone runtime permission is currently granted. */
interface MicPermissionChecker {
    fun isGranted(): Boolean
}

class AndroidMicPermissionChecker @Inject constructor(
    @ApplicationContext private val context: Context
) : MicPermissionChecker {
    override fun isGranted(): Boolean =
        ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO) ==
            PackageManager.PERMISSION_GRANTED
}

/** Thrown by `VoiceRepository.joinChannel` when RECORD_AUDIO has not been granted. */
class MissingMicPermissionException : IllegalStateException("RECORD_AUDIO not granted")
