package io.wolftown.kaiku.ui.channel

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class EndFollowerTest {

    /** Brings [follower] into the "user scrolled up" state, so following must be earned back. */
    private fun scrolledUp(): EndFollower = EndFollower().apply {
        onScrollState(scrolling = true, canScrollForward = true)
        onScrollState(scrolling = false, canScrollForward = true)
        check(!shouldReturnToEnd(scrolling = false, canScrollForward = true))
    }

    @Test
    fun `scrolled up, growth below never moves the list`() {
        val follower = scrolledUp()
        follower.onScrollState(scrolling = false, canScrollForward = true) // a message grew below
        assertFalse(follower.shouldReturnToEnd(scrolling = false, canScrollForward = true))
    }

    @Test
    fun `the user scrolling back to the end follows again`() {
        val follower = scrolledUp()
        follower.onScrollState(scrolling = true, canScrollForward = false) // reached the end
        follower.onScrollState(scrolling = false, canScrollForward = false)
        // "Sending…" turns into "not sent" with Retry and Delete: content grows below the view.
        follower.onScrollState(scrolling = false, canScrollForward = true)
        assertTrue(follower.shouldReturnToEnd(scrolling = false, canScrollForward = true))
    }

    @Test
    fun `an animated scroll of the screen that settles at the end follows again`() {
        val follower = scrolledUp()
        // Jump to newest / after send: every frame of the animation can still scroll forward ...
        follower.onScrollState(scrolling = true, canScrollForward = true)
        // ... and the list then settles at the end.
        follower.onScrollState(scrolling = false, canScrollForward = false)
        follower.onScrollState(scrolling = false, canScrollForward = true) // keyboard or snackbar opens
        assertTrue(follower.shouldReturnToEnd(scrolling = false, canScrollForward = true))
    }

    @Test
    fun `an instant jump to the end that is never seen as scrolling follows again`() {
        val follower = scrolledUp()
        follower.onScrollState(scrolling = false, canScrollForward = false)
        assertTrue(follower.shouldReturnToEnd(scrolling = false, canScrollForward = true))
    }

    @Test
    fun `never fights a scroll in progress`() {
        val follower = EndFollower()
        follower.onScrollState(scrolling = false, canScrollForward = false)
        assertFalse(follower.shouldReturnToEnd(scrolling = true, canScrollForward = true))
    }

    @Test
    fun `a jump away from the end stops following`() {
        val follower = EndFollower()
        follower.onScrollState(scrolling = false, canScrollForward = false)
        follower.leftEnd()
        assertFalse(follower.shouldReturnToEnd(scrolling = false, canScrollForward = true))
    }
}
