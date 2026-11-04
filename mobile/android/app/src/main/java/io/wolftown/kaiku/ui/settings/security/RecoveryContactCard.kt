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
        val name = contactName(contact.displayName, contact.username)
        Body(stringResource(if (contact.confirmedAt != null) R.string.security_contact_confirmed else R.string.security_contact_waiting, name))
    }
    if (!readOnly) {
        Action(stringResource(R.string.security_contact_choose), onClick = onChoose)
        if (contact != null) Action(stringResource(R.string.security_remove), onClick = onRemove)
    }
}

/**
 * A person as a recovery contact: the unique `@username` always, with the display name
 * (which anyone may change and two people may share) only next to it (Argon part8 #5).
 */
internal fun contactName(displayName: String, username: String): String =
    if (displayName.isNotBlank() && displayName != username) "$displayName (@$username)" else "@$username"
