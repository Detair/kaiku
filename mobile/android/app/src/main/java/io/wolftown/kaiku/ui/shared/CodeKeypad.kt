package io.wolftown.kaiku.ui.shared

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.wolftown.kaiku.R
import io.wolftown.kaiku.domain.CodeFormat

/**
 * A large digit keypad for 6-digit codes (spec Constraints, older users):
 * keys 0–9 as ≥ 64 dp buttons in a 3×4 grid with a labelled "Delete" key, the
 * grouped value (≥ 32 sp) above, "Confirm" enabled at 6 digits. It never
 * submits by itself; corrections are always possible. Every key has a
 * TalkBack label (its digit or "Delete").
 */
@Composable
fun CodeKeypad(value: String, onValueChange: (String) -> Unit, onConfirm: () -> Unit, enabled: Boolean = true) {
    val shown = CodeFormat.group(value).ifEmpty { "– – –  – – –" }
    val description = stringResource(R.string.keypad_value_description, CodeFormat.group(value))
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(
            text = shown,
            fontSize = 40.sp,
            fontWeight = FontWeight.Bold,
            fontFamily = FontFamily.Monospace,
            textAlign = TextAlign.Center,
            color = MaterialTheme.colorScheme.onBackground,
            modifier = Modifier.fillMaxWidth().semantics {
                contentDescription = description
                liveRegion = LiveRegionMode.Polite
            },
        )
        val rows = listOf(listOf('1', '2', '3'), listOf('4', '5', '6'), listOf('7', '8', '9'))
        rows.forEach { row ->
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp), modifier = Modifier.fillMaxWidth()) {
                row.forEach { d -> DigitKey(d, enabled) { onValueChange(CodeFormat.accept(value, d)) } }
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp), modifier = Modifier.fillMaxWidth()) {
            OutlinedButton(
                onClick = { onValueChange(CodeFormat.delete(value)) },
                enabled = enabled && value.isNotEmpty(),
                modifier = Modifier.weight(1f).heightIn(min = KeyScreenSizes.primaryButton),
            ) {
                Text(stringResource(R.string.keypad_delete), fontSize = 18.sp, color = MaterialTheme.colorScheme.onBackground)
            }
            DigitKey('0', enabled) { onValueChange(CodeFormat.accept(value, '0')) }
            Spacer(Modifier.weight(1f))
        }
        KeyPrimaryButton(
            text = stringResource(R.string.keypad_confirm),
            onClick = onConfirm,
            enabled = enabled && CodeFormat.isComplete(value),
        )
    }
}

@Composable
private fun androidx.compose.foundation.layout.RowScope.DigitKey(d: Char, enabled: Boolean, onClick: () -> Unit) {
    FilledTonalButton(
        onClick = onClick,
        enabled = enabled,
        modifier = Modifier.weight(1f).heightIn(min = KeyScreenSizes.primaryButton),
    ) {
        Text(d.toString(), fontSize = 28.sp, color = MaterialTheme.colorScheme.onSecondaryContainer)
    }
}
