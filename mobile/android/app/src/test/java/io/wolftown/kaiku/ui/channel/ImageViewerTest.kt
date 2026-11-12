package io.wolftown.kaiku.ui.channel

import org.junit.Assert.assertEquals
import org.junit.Test

class ImageViewerTest {

    @Test
    fun `a photo up to the cap is decoded at its own size`() {
        assertEquals(DecodeSize(2400, 1600), viewerDecodeSize(2400, 1600))
        assertEquals(DecodeSize(4096, 100), viewerDecodeSize(4096, 100))
    }

    @Test
    fun `a larger photo is scaled so its longest side is the cap`() {
        assertEquals(DecodeSize(4096, 2731), viewerDecodeSize(6000, 4000))
        assertEquals(DecodeSize(2731, 4096), viewerDecodeSize(4000, 6000))
        assertEquals(DecodeSize(4096, 1), viewerDecodeSize(100_000, 10))
    }

    @Test
    fun `unknown dimensions decode up to the cap on both sides`() {
        assertEquals(DecodeSize(MAX_VIEWER_DECODE_PX, MAX_VIEWER_DECODE_PX), viewerDecodeSize(null, 1600))
        assertEquals(DecodeSize(MAX_VIEWER_DECODE_PX, MAX_VIEWER_DECODE_PX), viewerDecodeSize(0, 0))
    }

    @Test
    fun `zoom stays between 1 and the maximum`() {
        assertEquals(1f, clampZoom(1f, 0.5f))
        assertEquals(5f, clampZoom(4f, 2f))
    }
}
