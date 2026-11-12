package io.wolftown.kaiku.ui.channel

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Test

/** Review W2: the second and third message of the same person are announced too. */
@OptIn(ExperimentalCoroutinesApi::class)
class LiveAnnouncerTest {

    @Test
    fun `the same words twice are two separate changes`() = runTest {
        val announcer = LiveAnnouncer(gapMs = 100, holdMs = 4_000)
        val seen = mutableListOf<String>()
        backgroundScope.launch { announcer.run() }
        backgroundScope.launch { announcer.text.collect { seen += it } }
        runCurrent()

        announcer.announce("New message from Anna")
        advanceTimeBy(200)
        assertEquals("New message from Anna", announcer.text.value)

        announcer.announce("New message from Anna")
        runCurrent()
        assertEquals("", announcer.text.value) // emptied first ...
        advanceTimeBy(200)
        assertEquals("New message from Anna", announcer.text.value) // ... then said again

        assertEquals(2, seen.count { it == "New message from Anna" })
    }

    @Test
    fun `the region is emptied after the announcement`() = runTest {
        val announcer = LiveAnnouncer(gapMs = 100, holdMs = 4_000)
        backgroundScope.launch { announcer.run() }
        runCurrent()

        announcer.announce("2 new messages")
        advanceTimeBy(5_000)

        assertEquals("", announcer.text.value)
    }
}
