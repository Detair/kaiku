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
private val NordOnSecondaryContainer = Color(0xFF81A1C1)
private val NordTertiary = Color(0xFFA3BE8C)
private val NordOnTertiary = Color(0xFF242933)
private val NordTertiaryContainer = Color(0xFF3B4252)
private val NordOnTertiaryContainer = Color(0xFFA3BE8C)
private val NordError = Color(0xFFBF616A)
private val NordOnError = Color(0xFFECEFF4)
private val NordErrorContainer = Color(0xFF3A2D2F)
private val NordOnErrorContainer = Color(0xFFF0A0A8)
private val NordSurfaceBase = Color(0xFF242933)
private val NordSurfaceLayer1 = Color(0xFF2E3440)
private val NordSurfaceLayer2 = Color(0xFF3B4252)
private val NordSurfaceHighlight = Color(0xFF434C5E)
private val NordTextPrimary = Color(0xFFECEFF4)
private val NordTextSecondary = Color(0xFFD8DEE9)
private val NordOutline = Color(0xFF4C566A)
private val NordOutlineVariant = Color(0xFF3D4350)

private val KaikuDarkColorScheme = darkColorScheme(
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
    inversePrimary = Color(0xFF5E81AC),
    surfaceDim = NordSurfaceBase,
    surfaceContainer = NordSurfaceLayer1,
    surfaceContainerLow = NordSurfaceBase,
    surfaceContainerHigh = NordSurfaceLayer2,
    surfaceContainerHighest = NordSurfaceHighlight
)

private val KaikuLightColorScheme = lightColorScheme(
    primary = Color(0xFF2E6D7E),
    onPrimary = Color.White,
    primaryContainer = Color(0xFFB8E0EB),
    onPrimaryContainer = Color(0xFF1A4050),
    secondary = Color(0xFF4C6A8A),
    onSecondary = Color.White,
    tertiary = Color(0xFF5A7A4E),
    onTertiary = Color.White,
    error = Color(0xFFBF616A),
    onError = Color.White,
    background = Color(0xFFF8F9FC),
    onBackground = Color(0xFF2E3440),
    surface = Color(0xFFF0F2F6),
    onSurface = Color(0xFF2E3440),
    surfaceVariant = Color(0xFFE5E9F0),
    onSurfaceVariant = Color(0xFF4C566A),
    outline = Color(0xFF8892A3)
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
