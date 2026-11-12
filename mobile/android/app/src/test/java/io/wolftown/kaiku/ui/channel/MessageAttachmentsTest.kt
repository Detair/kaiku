package io.wolftown.kaiku.ui.channel

import org.junit.Assert.assertEquals
import org.junit.Test

class MessageAttachmentsTest {

    @Test
    fun `a landscape photo fills the width of the tile`() {
        assertEquals(PreviewTile(240, 160), previewTile(2400, 1600))
    }

    @Test
    fun `a portrait photo fills the height of the tile`() {
        assertEquals(PreviewTile(160, 240), previewTile(1600, 2400))
        assertEquals(PreviewTile(180, 240), previewTile(1500, 2000))
    }

    @Test
    fun `a small photo is not blown up`() {
        assertEquals(PreviewTile(200, 150), previewTile(200, 150))
    }

    @Test
    fun `very narrow photos keep a tile big enough for the error text and Retry`() {
        assertEquals(PreviewTile(240, PREVIEW_MIN_HEIGHT_DP), previewTile(4000, 100))
        assertEquals(PreviewTile(PREVIEW_MIN_WIDTH_DP, 240), previewTile(100, 4000))
        assertEquals(PreviewTile(PREVIEW_MIN_WIDTH_DP, PREVIEW_MIN_HEIGHT_DP), previewTile(10, 10))
    }

    @Test
    fun `unknown dimensions give a fixed tile`() {
        assertEquals(PreviewTile(240, 160), previewTile(null, null))
        assertEquals(PreviewTile(240, 160), previewTile(0, 600))
    }
}
