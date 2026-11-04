package io.wolftown.kaiku.ui.admin

import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.selection.selectable
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.sp
import io.wolftown.kaiku.R
import io.wolftown.kaiku.ui.shared.KeyBody
import io.wolftown.kaiku.ui.shared.KeyScreenSizes

/** C27 `claimant_check`: how the admin knows the person in front of them is the account's owner. */
object ClaimantCheck {
    const val IN_PERSON = "in_person"
    const val CALLED_BACK = "called_back"

    /** The note is required: 1–500 characters. */
    fun noteValid(note: String): Boolean = note.trim().length in 1..500
}

@Composable
fun ClaimantCheckStep(method: String, onMethod: (String) -> Unit, note: String, onNote: (String) -> Unit) {
    listOf(ClaimantCheck.IN_PERSON to R.string.admin_claimant_in_person, ClaimantCheck.CALLED_BACK to R.string.admin_claimant_called_back)
        .forEach { (value, label) ->
            Row(
                Modifier.fillMaxWidth().heightIn(min = KeyScreenSizes.secondaryButton)
                    .selectable(selected = method == value, onClick = { onMethod(value) }, role = Role.RadioButton),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                RadioButton(selected = method == value, onClick = null)
                Text(stringResource(label), fontSize = 18.sp, color = MaterialTheme.colorScheme.onBackground)
            }
        }
    if (method == ClaimantCheck.CALLED_BACK) KeyBody(stringResource(R.string.admin_claimant_called_back_explain), secondary = true)
    OutlinedTextField(
        value = note,
        onValueChange = { onNote(it.take(500)) },
        label = { Text(stringResource(R.string.admin_claimant_note)) },
        modifier = Modifier.fillMaxWidth(),
        minLines = 2,
    )
}
