package io.wolftown.kaiku.ui.channel

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class MessageAttachmentsTest {

    // --- the loaded photo: its own shape, at most 240 dp, never upscaled ---

    @Test
    fun `a large landscape photo fills the width`() {
        assertEquals(PreviewTile(240, 160), photoSize(2400, 1600, density = 1.33f))
    }

    @Test
    fun `a large portrait photo fills the height`() {
        assertEquals(PreviewTile(180, 240), photoSize(1500, 2000, density = 2f))
    }

    @Test
    fun `a small photo is drawn at no more than its own pixels`() {
        // 200 x 150 px at 1.33 px per dp is 150 x 113 dp, not 200 x 150 dp.
        assertEquals(PreviewTile(150, 113), photoSize(200, 150, density = 1.33f))
        assertEquals(PreviewTile(200, 150), photoSize(200, 150, density = 1f))
    }

    @Test
    fun `a panorama keeps its shape without a minimum height`() {
        assertEquals(PreviewTile(240, 6), photoSize(4000, 100, density = 1f))
        assertEquals(PreviewTile(240, 1), photoSize(100_000, 10, density = 1f))
    }

    @Test
    fun `unknown dimensions give no photo size`() {
        assertNull(photoSize(null, 1600, density = 1f))
        assertNull(photoSize(0, 600, density = 1f))
    }

    // --- the loading / failed tile: same size where possible, but room for text and Retry ---

    @Test
    fun `the loading tile has the photo's size when that is big enough`() {
        assertEquals(PreviewTile(240, 160), placeholderTile(PreviewTile(240, 160)))
    }

    @Test
    fun `the loading tile keeps a minimum for narrow or tiny photos`() {
        assertEquals(PreviewTile(240, PREVIEW_MIN_HEIGHT_DP), placeholderTile(PreviewTile(240, 6)))
        assertEquals(PreviewTile(PREVIEW_MIN_WIDTH_DP, PREVIEW_MIN_HEIGHT_DP), placeholderTile(PreviewTile(8, 8)))
    }

    @Test
    fun `the loading tile of a photo without dimensions is fixed`() {
        assertEquals(PreviewTile(240, 160), placeholderTile(null))
    }
}
