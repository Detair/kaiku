package io.wolftown.kaiku.navigation

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class DeepLinksTest {
    private val id = "123e4567-e89b-12d3-a456-426614174000"

    @Test fun `channel deep link round trips`() {
        assertEquals(id, DeepLinks.parseChannelId(DeepLinks.channelUri(id)))
    }

    @Test fun `deep link with invalid id is ignored`() {
        assertNull(DeepLinks.parseChannelId("kaiku://channel/../x"))
        assertNull(DeepLinks.parseChannelId("kaiku://auth/callback"))
        assertNull(DeepLinks.parseChannelId("https://evil/channel/$id"))
        assertNull(DeepLinks.parseChannelId("kaiku://channel/$id/extra"))
        assertNull(DeepLinks.parseChannelId("kaiku://channel/1-2-3-4-5"))
        assertNull(DeepLinks.parseChannelId(null))
    }
}
