package io.wolftown.kaiku.ui.shared

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * Building blocks for the phone-as-key screens, sized for older users (spec
 * Constraints): approval, setup and keypad buttons ≥ 64 dp, other actions
 * ≥ 56 dp, body text ≥ 18 sp, all in sp so the font scale applies. Readable
 * text uses onBackground/onSurfaceVariant, never an accent colour.
 */
object KeyScreenSizes {
    val primaryButton: Dp = 64.dp
    val secondaryButton: Dp = 56.dp
}

@Composable
fun KeyScreen(modifier: Modifier = Modifier, content: @Composable ColumnScope.() -> Unit) {
    Column(
        modifier = modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 24.dp, vertical = 32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(16.dp, Alignment.CenterVertically),
        content = content,
    )
}

@Composable
fun KeyTitle(text: String, modifier: Modifier = Modifier) {
    Text(
        text = text,
        style = MaterialTheme.typography.headlineMedium,
        color = MaterialTheme.colorScheme.onBackground,
        textAlign = TextAlign.Center,
        modifier = modifier.fillMaxWidth(),
    )
}

@Composable
fun KeyBody(text: String, modifier: Modifier = Modifier, secondary: Boolean = false) {
    Text(
        text = text,
        fontSize = 18.sp,
        lineHeight = 26.sp,
        color = if (secondary) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.onBackground,
        textAlign = TextAlign.Center,
        modifier = modifier.fillMaxWidth(),
    )
}

/** A notice with an icon and text, so severity never relies on colour alone. */
@Composable
fun KeyNotice(text: String, warning: Boolean, modifier: Modifier = Modifier) {
    Row(modifier = modifier.fillMaxWidth(), verticalAlignment = Alignment.Top) {
        Icon(
            imageVector = if (warning) Icons.Filled.Warning else Icons.Filled.Info,
            contentDescription = null,
            tint = if (warning) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(top = 2.dp).size(24.dp),
        )
        Spacer(Modifier.width(12.dp))
        Text(text = text, fontSize = 18.sp, lineHeight = 26.sp, color = MaterialTheme.colorScheme.onBackground)
    }
}

/** The main action: filled, ≥ 64 dp high, full width. */
@Composable
fun KeyPrimaryButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    working: Boolean = false,
    minHeight: Dp = KeyScreenSizes.primaryButton,
) {
    Button(
        onClick = onClick,
        enabled = enabled && !working,
        modifier = modifier.fillMaxWidth().heightIn(min = minHeight),
        colors = ButtonDefaults.buttonColors(),
    ) {
        if (working) {
            CircularProgressIndicator(modifier = Modifier.size(24.dp), strokeWidth = 2.dp)
            Spacer(Modifier.width(12.dp))
        }
        Text(text = text, fontSize = 20.sp, textAlign = TextAlign.Center)
    }
}

/** A second action: outlined, ≥ 56 dp (≥ 64 dp where [minHeight] says so). */
@Composable
fun KeySecondaryButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    minHeight: Dp = KeyScreenSizes.secondaryButton,
) {
    OutlinedButton(
        onClick = onClick,
        enabled = enabled,
        modifier = modifier.fillMaxWidth().heightIn(min = minHeight),
    ) {
        Text(text = text, fontSize = 18.sp, textAlign = TextAlign.Center, color = MaterialTheme.colorScheme.onBackground)
    }
}

/** The FragmentActivity host for BiometricPrompt (L2-A2), or null in previews. */
@Composable
fun currentFragmentActivity(): androidx.fragment.app.FragmentActivity? =
    androidx.activity.compose.LocalActivity.current as? androidx.fragment.app.FragmentActivity
