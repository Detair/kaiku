package io.wolftown.kaiku.ui.shared

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp

/**
 * Draws [payload] as a QR code, at least 240 dp wide, always black modules on
 * white (whatever the theme), so every phone camera reads it.
 */
@Composable
fun QrImage(payload: String, contentDescription: String, modifier: Modifier = Modifier) {
    val matrix = remember(payload) { qrMatrix(payload) }
    Canvas(
        modifier = modifier
            .sizeIn(minWidth = 240.dp, minHeight = 240.dp)
            .aspectRatio(1f)
            .background(Color.White)
            .semantics { this.contentDescription = contentDescription },
    ) {
        val cell = size.width / matrix.width
        for (y in 0 until matrix.height) {
            for (x in 0 until matrix.width) {
                if (matrix.get(x, y)) {
                    drawRect(
                        color = Color.Black,
                        topLeft = Offset(x * cell, y * cell),
                        // A hair larger, so no seams show between neighbouring modules.
                        size = Size(cell + 0.5f, cell + 0.5f),
                    )
                }
            }
        }
    }
}
