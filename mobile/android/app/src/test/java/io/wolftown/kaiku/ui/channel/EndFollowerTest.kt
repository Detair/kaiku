package io.wolftown.kaiku.ui.channel

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class EndFollowerTest {

    @Test
    fun `at the end, a last message that grows is brought back into view`() {
        val follower = EndFollower()
        // At the end of the chat, nothing more below.
        follower.onScrollState(scrolling = false, canScrollForward = false)
        // "Sending…" turns into "not sent" with Retry and Delete: content grows below the view.
        follower.onScrollState(scrolling = false, canScrollForward = true)
        assertTrue(follower.shouldReturnToEnd(scrolling = false, canScrollForward = true))
    }

    @Test
    fun `at the end, a smaller view (keyboard, snackbar) returns to the end`() {
        val follower = EndFollower()
        follower.onScrollState(scrolling = true, canScrollForward = false) // user scrolled to the end
        follower.onScrollState(scrolling = false, canScrollForward = false)
        follower.onScrollState(scrolling = false, canScrollForward = true) // the view shrank
        assertTrue(follower.shouldReturnToEnd(scrolling = false, canScrollForward = true))
    }

    @Test
    fun `scrolled up, growth below never moves the list`() {
        val follower = EndFollower()
        follower.onScrollState(scrolling = true, canScrollForward = true) // user drags up
        follower.onScrollState(scrolling = false, canScrollForward = true)
        assertFalse(follower.shouldReturnToEnd(scrolling = false, canScrollForward = true))
    }

    @Test
    fun `scrolling back down to the end follows again`() {
        val follower = EndFollower()
        follower.onScrollState(scrolling = true, canScrollForward = true)
        follower.onScrollState(scrolling = true, canScrollForward = false) // reached the end
        follower.onScrollState(scrolling = false, canScrollForward = false)
        follower.onScrollState(scrolling = false, canScrollForward = true)
        assertTrue(follower.shouldReturnToEnd(scrolling = false, canScrollForward = true))
    }

    @Test
    fun `never fights a scroll in progress`() {
        val follower = EndFollower()
        assertFalse(follower.shouldReturnToEnd(scrolling = true, canScrollForward = true))
    }

    @Test
    fun `a jump away from the end stops following`() {
        val follower = EndFollower()
        follower.leftEnd()
        assertFalse(follower.shouldReturnToEnd(scrolling = false, canScrollForward = true))
    }
}
