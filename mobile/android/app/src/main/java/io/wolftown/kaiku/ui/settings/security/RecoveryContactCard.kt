package io.wolftown.kaiku.ui.settings.security

import androidx.compose.runtime.Composable
import androidx.compose.ui.res.stringResource
import io.wolftown.kaiku.R
import io.wolftown.kaiku.domain.model.RecoveryContact

@Composable
fun RecoveryContactCard(contact: RecoveryContact?, readOnly: Boolean, onChoose: () -> Unit, onRemove: () -> Unit) {
    if (contact == null) {
        Body(stringResource(R.string.security_contact_none))
    } else {
        val name = contact.displayName.ifEmpty { contact.username }
        Body(stringResource(if (contact.confirmedAt != null) R.string.security_contact_confirmed else R.string.security_contact_waiting, name))
    }
    if (!readOnly) {
        Action(stringResource(R.string.security_contact_choose), onClick = onChoose)
        if (contact != null) Action(stringResource(R.string.security_remove), onClick = onRemove)
    }
}
