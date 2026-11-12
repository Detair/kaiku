package io.wolftown.kaiku.ui.channel

import android.net.Uri
import androidx.annotation.StringRes
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.repository.AttachmentFiles
import io.wolftown.kaiku.domain.model.Attachment
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.launch
import java.io.File
import java.util.logging.Logger
import javax.inject.Inject
import kotlin.coroutines.cancellation.CancellationException

/** What the screen does next for an attachment (AND-B-4). */
sealed interface AttachmentEvent {
    /** Show [uri] with another app (video, document). */
    data class Open(val uri: Uri, val mimeType: String) : AttachmentEvent

    /** Offer [uri] to the system share sheet. */
    data class Share(val uri: Uri, val mimeType: String) : AttachmentEvent

    /** Before Android 10: ask where to save with the system dialog, then call [AttachmentViewModel.saveTo]. */
    data class ChooseSaveTarget(val fileName: String, val mimeType: String) : AttachmentEvent

    /**
     * A short message for the user. [origin] is the photo viewer that started
     * the action (see [showInViewer]); null when it came from the chat list.
     */
    data class Message(@StringRes val text: Int, val origin: Long? = null) : AttachmentEvent
}

/**
 * True when a result belongs in the photo viewer: the viewer that started the
 * action ([origin]) is the one open now ([openViewer]). Once it is closed the
 * chat shows it; another photo's viewer never does.
 */
internal fun showInViewer(origin: Long?, openViewer: Long?): Boolean = origin != null && origin == openViewer

/** Opens, shares and saves received attachments; one action at a time. */
@HiltViewModel
class AttachmentViewModel @Inject constructor(
    private val files: AttachmentFiles,
) : ViewModel() {

    private companion object {
        val logger: Logger = Logger.getLogger("AttachmentViewModel")
    }

    private val _busy = MutableStateFlow(false)

    /** True while a file is downloaded or saved. */
    val busy: StateFlow<Boolean> = _busy.asStateFlow()

    private val _events = Channel<AttachmentEvent>(Channel.BUFFERED)
    val events: Flow<AttachmentEvent> = _events.receiveAsFlow()

    /** The file waiting for a target from the "save as" dialog. */
    private var pendingSave: File? = null
    private var pendingSaveOrigin: Long? = null

    fun open(attachment: Attachment) = withFile(attachment, R.string.chat_attachment_open_failed) { file ->
        AttachmentEvent.Open(files.shareableUri(file), attachment.mimeType)
    }

    /** [origin]: the photo viewer this was started from, for [AttachmentEvent.Message.origin]. */
    fun share(attachment: Attachment, origin: Long? = null) =
        withFile(attachment, R.string.chat_attachment_open_failed, origin) { file ->
            AttachmentEvent.Share(files.shareableUri(file), attachment.mimeType)
        }

    /** [origin]: the photo viewer this was started from, for [AttachmentEvent.Message.origin]. */
    fun save(attachment: Attachment, origin: Long? = null) =
        withFile(attachment, R.string.chat_attachment_save_failed, origin) { file ->
            if (files.saveToDevice(file, attachment)) {
                AttachmentEvent.Message(R.string.chat_attachment_saved, origin)
            } else {
                pendingSave = file
                pendingSaveOrigin = origin
                AttachmentEvent.ChooseSaveTarget(file.name, attachment.mimeType)
            }
        }

    /** The user picked [target] in the "save as" dialog (null: cancelled). */
    fun saveTo(target: Uri?) {
        val file = pendingSave ?: return
        pendingSave = null
        val origin = pendingSaveOrigin
        pendingSaveOrigin = null
        if (target == null) return
        run(R.string.chat_attachment_save_failed, origin) {
            files.copyTo(file, target)
            AttachmentEvent.Message(R.string.chat_attachment_saved, origin)
        }
    }

    private fun withFile(
        attachment: Attachment,
        @StringRes failure: Int,
        origin: Long? = null,
        next: suspend (File) -> AttachmentEvent,
    ) = run(failure, origin) { next(files.download(attachment)) }

    private fun run(@StringRes failure: Int, origin: Long? = null, block: suspend () -> AttachmentEvent) {
        if (_busy.value) return
        _busy.value = true
        viewModelScope.launch {
            val event = try {
                block()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                logger.warning("attachment action failed (${e::class.simpleName})")
                AttachmentEvent.Message(failure, origin)
            } finally {
                _busy.value = false
            }
            _events.send(event)
        }
    }
}
