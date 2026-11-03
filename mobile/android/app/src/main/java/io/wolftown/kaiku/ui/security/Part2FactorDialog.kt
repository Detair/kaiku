package io.wolftown.kaiku.ui.security

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.FilterChip
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.window.SecureFlagPolicy
import io.wolftown.kaiku.R
import io.wolftown.kaiku.domain.model.Part2Factor

/** Owner rule part 2 (B.2): a fresh authenticator code or a recovery code. */
@Composable
fun Part2FactorDialog(onSubmit: (Part2Factor) -> Unit, onDismiss: () -> Unit) {
    var recovery by remember { mutableStateOf(false) }
    var code by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        // A recovery or authenticator code is typed here: no screenshots, not cast (B.1).
        properties = DialogProperties(securePolicy = SecureFlagPolicy.SecureOn),
        title = { Text(stringResource(R.string.security_part2_title)) },
        text = {
            Column {
                Row {
                    FilterChip(selected = !recovery, onClick = { recovery = false }, label = { Text(stringResource(R.string.security_part2_totp)) },
                        modifier = Modifier.padding(end = 8.dp))
                    FilterChip(selected = recovery, onClick = { recovery = true }, label = { Text(stringResource(R.string.security_part2_recovery)) })
                }
                OutlinedTextField(
                    value = code,
                    onValueChange = { code = it.take(32) },
                    label = { Text(stringResource(if (recovery) R.string.security_part2_recovery else R.string.security_part2_totp)) },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = if (recovery) KeyboardType.Ascii else KeyboardType.NumberPassword),
                    modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
                )
                Text(stringResource(R.string.security_part2_hint), modifier = Modifier.padding(top = 8.dp))
                Text(stringResource(R.string.security_part2_counts))
            }
        },
        confirmButton = {
            TextButton(onClick = {
                val c = code.trim()
                if (c.isNotEmpty()) onSubmit(if (recovery) Part2Factor.recoveryCode(c) else Part2Factor.totp(c))
            }) { Text(stringResource(R.string.keypad_confirm)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.common_cancel)) } },
    )
}
