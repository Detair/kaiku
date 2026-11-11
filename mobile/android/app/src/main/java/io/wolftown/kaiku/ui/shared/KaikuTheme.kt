package io.wolftown.kaiku.ui.shared

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext

// Nord-based color palette matching the Kaiku desktop "Focused Hybrid" theme
private val NordPrimary = Color(0xFF88C0D0)
private val NordOnPrimary = Color(0xFF242933)
private val NordPrimaryContainer = Color(0xFF3B4252)
private val NordOnPrimaryContainer = Color(0xFF88C0D0)
private val NordSecondary = Color(0xFF81A1C1)
private val NordOnSecondary = Color(0xFF242933)
private val NordSecondaryContainer = Color(0xFF3B4252)
// Text on the #3B4252 containers: #81A1C1 reached only 3.7:1 (AND-B-13), primary text reaches 8.7:1.
private val NordOnSecondaryContainer = Color(0xFFECEFF4)
private val NordTertiary = Color(0xFFA3BE8C)
private val NordOnTertiary = Color(0xFF242933)
private val NordTertiaryContainer = Color(0xFF3B4252)
private val NordOnTertiaryContainer = Color(0xFFA3BE8C)
// AND-B-13: #BF616A was 3.6:1 on the background and 3.1:1 on surfaces. The lighter red reaches
// 6.3:1 / 5.4:1 as text and 4.4:1 on surfaceVariant (icons); text on it is dark (6.3:1).
private val NordError = Color(0xFFE5959C)
private val NordOnError = Color(0xFF242933)
private val NordErrorContainer = Color(0xFF3A2D2F)
private val NordOnErrorContainer = Color(0xFFF0A0A8)
private val NordSurfaceBase = Color(0xFF242933)
private val NordSurfaceLayer1 = Color(0xFF2E3440)
private val NordSurfaceLayer2 = Color(0xFF3B4252)
private val NordSurfaceHighlight = Color(0xFF434C5E)
private val NordTextPrimary = Color(0xFFECEFF4)
private val NordTextSecondary = Color(0xFFD8DEE9)
// Field borders need 3:1: #4C566A was 2.0:1 on the background; this is 3.6:1 (3.1:1 on surface).
private val NordOutline = Color(0xFF737F96)
private val NordOutlineVariant = Color(0xFF3D4350)

internal val KaikuDarkColorScheme = darkColorScheme(
    primary = NordPrimary,
    onPrimary = NordOnPrimary,
    primaryContainer = NordPrimaryContainer,
    onPrimaryContainer = NordOnPrimaryContainer,
    secondary = NordSecondary,
    onSecondary = NordOnSecondary,
    secondaryContainer = NordSecondaryContainer,
    onSecondaryContainer = NordOnSecondaryContainer,
    tertiary = NordTertiary,
    onTertiary = NordOnTertiary,
    tertiaryContainer = NordTertiaryContainer,
    onTertiaryContainer = NordOnTertiaryContainer,
    error = NordError,
    onError = NordOnError,
    errorContainer = NordErrorContainer,
    onErrorContainer = NordOnErrorContainer,
    background = NordSurfaceBase,
    onBackground = NordTextPrimary,
    surface = NordSurfaceLayer1,
    onSurface = NordTextPrimary,
    surfaceVariant = NordSurfaceLayer2,
    onSurfaceVariant = NordTextSecondary,
    outline = NordOutline,
    outlineVariant = NordOutlineVariant,
    inverseSurface = NordTextPrimary,
    inverseOnSurface = NordSurfaceBase,
    // Snackbar action text on inverseSurface (#ECEFF4): #5E81AC was 3.5:1, this is 4.9:1.
    inversePrimary = Color(0xFF4C6A8A),
    surfaceDim = NordSurfaceBase,
    surfaceContainer = NordSurfaceLayer1,
    surfaceContainerLow = NordSurfaceBase,
    surfaceContainerHigh = NordSurfaceLayer2,
    surfaceContainerHighest = NordSurfaceHighlight
)

internal val KaikuLightColorScheme = lightColorScheme(
    primary = Color(0xFF2E6D7E),
    onPrimary = Color.White,
    primaryContainer = Color(0xFFB8E0EB),
    onPrimaryContainer = Color(0xFF1A4050),
    secondary = Color(0xFF4C6A8A),
    onSecondary = Color.White,
    tertiary = Color(0xFF5A7A4E),
    onTertiary = Color.White,
    // 5.6:1 on the light background; white on it 5.9:1 (#BF616A: 3.9:1 / 4.1:1).
    error = Color(0xFFA8434D),
    onError = Color.White,
    background = Color(0xFFF8F9FC),
    onBackground = Color(0xFF2E3440),
    surface = Color(0xFFF0F2F6),
    onSurface = Color(0xFF2E3440),
    surfaceVariant = Color(0xFFE5E9F0),
    onSurfaceVariant = Color(0xFF4C566A),
    // Field borders need 3:1 on the background (#8892A3 was 3.0:1 and below on surfaces).
    outline = Color(0xFF6F7A8C),
    inversePrimary = Color(0xFF88C0D0),
)

@Composable
fun KaikuTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    dynamicColor: Boolean = false,
    content: @Composable () -> Unit
) {
    val colorScheme = when {
        dynamicColor && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S -> {
            val context = LocalContext.current
            if (darkTheme) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
        }
        darkTheme -> KaikuDarkColorScheme
        else -> KaikuLightColorScheme
    }

    MaterialTheme(
        colorScheme = colorScheme,
        content = content
    )
}
