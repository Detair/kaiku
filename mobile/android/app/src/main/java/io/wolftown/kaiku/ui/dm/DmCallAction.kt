package io.wolftown.kaiku.ui.dm

import android.Manifest
import android.content.pm.PackageManager
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Call
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.core.content.ContextCompat
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.call.CallController
import io.wolftown.kaiku.data.call.CallInput
import io.wolftown.kaiku.data.call.CallState
import io.wolftown.kaiku.data.call.signedInUserId
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.repository.DmRepository
import io.wolftown.kaiku.domain.model.DmConversation
import io.wolftown.kaiku.ui.call.CallIntents
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import javax.inject.Inject

/** What the call button of a conversation does. */
enum class DmCallButton {
    /** Not a DM with someone to call (a server channel, the Kaiku DM, alone in it). */
    Hidden,

    /** Starts a call. */
    Start,

    /** A call of this DM is on: back to the call screen. */
    Return,

    /** Another call is on or rings: no second call (shown, not usable). */
    Busy,
}

object DmCallButtons {
    fun of(channelId: String, dm: DmConversation?, me: String?, call: CallState): DmCallButton {
        if (dm == null || me == null || dm.isSystem || dm.readOnly) return DmCallButton.Hidden
        if (dm.participants.none { it.userId != me && !it.isSystem }) return DmCallButton.Hidden
        return when (call) {
            CallState.Idle, is CallState.Ended -> DmCallButton.Start
            else -> if (call.channelId == channelId) DmCallButton.Return else DmCallButton.Busy
        }
    }
}

@HiltViewModel
class DmCallViewModel @Inject constructor(
    savedStateHandle: SavedStateHandle,
    dmRepository: DmRepository,
    private val authState: AuthState,
    private val callController: CallController,
) : ViewModel() {
    private val channelId: String = savedStateHandle["channelId"]!!

    val button: StateFlow<DmCallButton> = combine(
        dmRepository.conversations,
        authState.currentUserId,
        callController.state,
    ) { dms, me, call ->
        DmCallButtons.of(channelId, dms.firstOrNull { it.id == channelId }, me, call)
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), DmCallButton.Hidden)

    /** Starts the call (the microphone permission is granted). */
    fun start() {
        val account = authState.signedInUserId() ?: return
        if (button.value == DmCallButton.Start) callController.submit(CallInput.Start(account, channelId))
    }
}

/**
 * The call button in a conversation's top bar (1:1 and group DMs, never the
 * Kaiku DM). Asks for the microphone first; then starts the call and opens
 * the call screen. During a call of this DM it leads back to the call screen.
 */
@Composable
fun DmCallAction(viewModel: DmCallViewModel = hiltViewModel()) {
    val button by viewModel.button.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val startCall = {
        viewModel.start()
        context.startActivity(CallIntents.screen(context))
    }
    val micPermission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        if (granted) {
            startCall()
        } else {
            Toast.makeText(context, R.string.voice_call_mic_needed, Toast.LENGTH_LONG).show()
        }
    }
    if (button == DmCallButton.Hidden) return
    IconButton(
        enabled = button != DmCallButton.Busy,
        onClick = {
            when {
                button == DmCallButton.Return -> context.startActivity(CallIntents.screen(context))
                ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO) ==
                    PackageManager.PERMISSION_GRANTED -> startCall()
                else -> micPermission.launch(Manifest.permission.RECORD_AUDIO)
            }
        },
    ) {
        Icon(Icons.Filled.Call, contentDescription = stringResource(R.string.voice_call_start_description))
    }
}
