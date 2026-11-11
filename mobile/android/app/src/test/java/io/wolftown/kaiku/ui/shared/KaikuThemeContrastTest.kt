package io.wolftown.kaiku.ui.shared

import androidx.compose.material3.ColorScheme
import androidx.compose.ui.graphics.Color
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow

/**
 * AND-B-13: error text, badges, banners, snackbar actions and red buttons reach
 * WCAG AA in both schemes (text 4.5:1, controls and icons 3:1).
 */
class KaikuThemeContrastTest {

    private fun channel(c: Float): Double =
        if (c <= 0.03928f) c / 12.92 else ((c + 0.055) / 1.055).pow(2.4)

    private fun luminance(color: Color): Double =
        0.2126 * channel(color.red) + 0.7152 * channel(color.green) + 0.0722 * channel(color.blue)

    private fun contrast(a: Color, b: Color): Double {
        val la = luminance(a)
        val lb = luminance(b)
        return (max(la, lb) + 0.05) / (min(la, lb) + 0.05)
    }

    private fun assertContrast(name: String, fg: Color, bg: Color, min: Double) {
        val ratio = contrast(fg, bg)
        assertTrue("$name: %.2f:1 < %.1f:1".format(ratio, min), ratio >= min)
    }

    private fun checkScheme(label: String, s: ColorScheme) {
        // Text (4.5:1)
        assertContrast("$label error text on background", s.error, s.background, 4.5)
        assertContrast("$label error text on surface", s.error, s.surface, 4.5)
        assertContrast("$label onError on error (badge, logout)", s.onError, s.error, 4.5)
        assertContrast("$label onErrorContainer", s.onErrorContainer, s.errorContainer, 4.5)
        assertContrast("$label banner text", s.onSecondaryContainer, s.secondaryContainer, 4.5)
        assertContrast("$label snackbar action", s.inversePrimary, s.inverseSurface, 4.5)
        assertContrast("$label snackbar text", s.inverseOnSurface, s.inverseSurface, 4.5)
        assertContrast("$label secondary text", s.onSurfaceVariant, s.surface, 4.5)
        assertContrast("$label secondary text on variant", s.onSurfaceVariant, s.surfaceVariant, 4.5)
        assertContrast("$label primary text", s.primary, s.background, 4.5)
        // Controls and icons (3:1)
        assertContrast("$label error icon on surfaceVariant (hang up)", s.error, s.surfaceVariant, 3.0)
        assertContrast("$label error button on background", s.error, s.background, 3.0)
        assertContrast("$label field outline", s.outline, s.background, 3.0)
    }

    @Test
    fun `dark scheme meets the contrast targets`() = checkScheme("dark", KaikuDarkColorScheme)

    @Test
    fun `light scheme meets the contrast targets`() = checkScheme("light", KaikuLightColorScheme)
}
