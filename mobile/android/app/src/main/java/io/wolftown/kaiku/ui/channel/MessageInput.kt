package io.wolftown.kaiku.ui.channel

import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Add
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import io.wolftown.kaiku.R

/**
 * Message composition area with typing indicators and send button.
 *
 * @param input Current message text
 * @param placeholder Hint shown while the input is empty
 * @param typingNames Display names of the users currently typing (never ids)
 * @param onInputChanged Callback when text changes
 * @param onSend Callback when the send button is pressed
 * @param isUploading True while an attachment uploads; disables the attach button
 * @param onAttachmentPicked Callback with the photo or video the user picked
 */
@Composable
fun MessageInput(
    input: String,
    placeholder: String,
    typingNames: List<String>,
    onInputChanged: (String) -> Unit,
    onSend: () -> Unit,
    isUploading: Boolean,
    onAttachmentPicked: (Uri) -> Unit,
    modifier: Modifier = Modifier
) {
    // The system photo picker needs no storage permission.
    val pickMedia = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        if (uri != null) onAttachmentPicked(uri)
    }

    Column(modifier = modifier) {
        // Typing indicator
        AnimatedVisibility(
            visible = typingNames.isNotEmpty(),
            enter = fadeIn(),
            exit = fadeOut()
        ) {
            Text(
                text = typingIndicatorText(typingNames),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp)
            )
        }

        // Input row
        Surface(
            tonalElevation = 2.dp,
            modifier = Modifier.fillMaxWidth()
        ) {
            Row(
                modifier = Modifier
                    .padding(horizontal = 8.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                IconButton(
                    onClick = {
                        pickMedia.launch(
                            PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageAndVideo)
                        )
                    },
                    enabled = !isUploading
                ) {
                    if (isUploading) {
                        CircularProgressIndicator(modifier = Modifier.size(20.dp), strokeWidth = 2.dp)
                    } else {
                        Icon(
                            imageVector = Icons.Filled.Add,
                            contentDescription = stringResource(R.string.chat_attach),
                            tint = MaterialTheme.colorScheme.onSurfaceVariant
                        )
                    }
                }

                TextField(
                    value = input,
                    onValueChange = onInputChanged,
                    placeholder = { Text(placeholder) },
                    modifier = Modifier.weight(1f),
                    singleLine = false,
                    maxLines = 5,
                    colors = TextFieldDefaults.colors(
                        focusedContainerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
                        unfocusedContainerColor = MaterialTheme.colorScheme.surfaceContainerHigh
                    )
                )

                Spacer(modifier = Modifier.width(8.dp))

                IconButton(
                    onClick = onSend,
                    enabled = input.isNotBlank()
                ) {
                    Icon(
                        imageVector = Icons.AutoMirrored.Filled.Send,
                        contentDescription = stringResource(R.string.chat_send),
                        tint = if (input.isNotBlank()) {
                            MaterialTheme.colorScheme.primary
                        } else {
                            MaterialTheme.colorScheme.onSurfaceVariant
                        }
                    )
                }
            }
        }
    }
}

/**
 * Typing indicator text for a set of usernames.
 *
 * - 1 user: "Alice is typing..."
 * - 2 users: "Alice and Bob are typing..."
 * - 3+ users: "Several people are typing..."
 */
@Composable
private fun typingIndicatorText(users: List<String>): String {
    return when (users.size) {
        0 -> ""
        1 -> stringResource(R.string.chat_typing_one, users[0])
        2 -> stringResource(R.string.chat_typing_two, users[0], users[1])
        else -> stringResource(R.string.chat_typing_many)
    }
}
