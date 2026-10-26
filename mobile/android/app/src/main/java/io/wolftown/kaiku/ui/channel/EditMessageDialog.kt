package io.wolftown.kaiku.ui.channel

import androidx.compose.material3.AlertDialog
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.res.stringResource
import io.wolftown.kaiku.R

/** Dialog for editing the text of an own message. Save is disabled for blank or unchanged text. */
@Composable
fun EditMessageDialog(
    initial: String,
    onConfirm: (String) -> Unit,
    onDismiss: () -> Unit
) {
    var text by remember(initial) { mutableStateOf(initial) }
    val canSave = text.isNotBlank() && text.trim() != initial.trim()

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.chat_edit_title)) },
        text = {
            OutlinedTextField(
                value = text,
                onValueChange = { text = it },
                label = { Text(stringResource(R.string.chat_edit_label)) },
                maxLines = 5
            )
        },
        confirmButton = {
            TextButton(onClick = { onConfirm(text) }, enabled = canSave) {
                Text(stringResource(R.string.common_save))
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) {
                Text(stringResource(R.string.common_cancel))
            }
        }
    )
}
