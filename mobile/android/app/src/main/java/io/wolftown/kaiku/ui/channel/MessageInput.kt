package io.wolftown.kaiku.ui.channel

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Send
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
 */
@Composable
fun MessageInput(
    input: String,
    placeholder: String,
    typingNames: List<String>,
    onInputChanged: (String) -> Unit,
    onSend: () -> Unit,
    modifier: Modifier = Modifier
) {
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
