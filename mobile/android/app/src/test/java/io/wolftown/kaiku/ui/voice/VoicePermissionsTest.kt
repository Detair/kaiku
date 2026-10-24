package io.wolftown.kaiku.ui.voice

import android.Manifest
import org.junit.Assert.assertEquals
import org.junit.Test

class VoicePermissionsTest {

    @Test
    fun `api 30 requests only record audio`() {
        assertEquals(listOf(Manifest.permission.RECORD_AUDIO), VoicePermissions.toRequest(30))
    }

    @Test
    fun `api 31 adds bluetooth connect`() {
        assertEquals(
            listOf(Manifest.permission.RECORD_AUDIO, Manifest.permission.BLUETOOTH_CONNECT),
            VoicePermissions.toRequest(31)
        )
    }

    @Test
    fun `api 33 adds post notifications`() {
        assertEquals(
            listOf(
                Manifest.permission.RECORD_AUDIO,
                Manifest.permission.BLUETOOTH_CONNECT,
                Manifest.permission.POST_NOTIFICATIONS
            ),
            VoicePermissions.toRequest(33)
        )
    }

    @Test
    fun `granted mic yields Join`() {
        assertEquals(
            VoicePermissionDecision.Join,
            VoicePermissions.decide(micGranted = true, shouldShowRationale = false, askedBefore = false)
        )
    }

    @Test
    fun `never asked yields Request`() {
        assertEquals(
            VoicePermissionDecision.Request,
            VoicePermissions.decide(micGranted = false, shouldShowRationale = false, askedBefore = false)
        )
    }

    @Test
    fun `first denial yields ShowRationale`() {
        assertEquals(
            VoicePermissionDecision.ShowRationale,
            VoicePermissions.decide(micGranted = false, shouldShowRationale = true, askedBefore = true)
        )
    }

    @Test
    fun `permanently denied mic yields OpenSettings decision`() {
        assertEquals(
            VoicePermissionDecision.OpenSettings,
            VoicePermissions.decide(micGranted = false, shouldShowRationale = false, askedBefore = true)
        )
    }
}
