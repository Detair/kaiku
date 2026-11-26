package io.wolftown.kaiku.ui.dm

import io.wolftown.kaiku.data.call.CallEndReason
import io.wolftown.kaiku.data.call.CallState
import io.wolftown.kaiku.domain.model.DmConversation
import io.wolftown.kaiku.domain.model.DmParticipant
import org.junit.Assert.assertEquals
import org.junit.Test

class DmCallButtonsTest {
    private val me = DmParticipant("me", "me", "Me")
    private val anna = DmParticipant("anna", "anna", "Anna")
    private val dm = DmConversation("dm-1", participants = listOf(me, anna))

    @Test
    fun `a DM with someone can be called`() {
        assertEquals(DmCallButton.Start, DmCallButtons.of("dm-1", dm, "me", CallState.Idle))
        assertEquals(DmCallButton.Start, DmCallButtons.of("dm-1", dm, "me", CallState.Ended("dm-1", CallEndReason.Left)))
    }

    @Test
    fun `no button outside DMs, in the Kaiku DM or alone`() {
        assertEquals(DmCallButton.Hidden, DmCallButtons.of("ch-1", null, "me", CallState.Idle))
        val kaiku = DmConversation("dm-k", participants = listOf(me, DmParticipant("k", "kaiku", "Kaiku", isSystem = true)), isSystem = true, readOnly = true)
        assertEquals(DmCallButton.Hidden, DmCallButtons.of("dm-k", kaiku, "me", CallState.Idle))
        assertEquals(DmCallButton.Hidden, DmCallButtons.of("dm-1", dm.copy(participants = listOf(me)), "me", CallState.Idle))
        assertEquals(DmCallButton.Hidden, DmCallButtons.of("dm-1", dm, null, CallState.Idle))
    }

    @Test
    fun `during a call this DM returns to it and another DM is busy`() {
        val call = CallState.Connected("dm-1", "c-1", 0L)
        assertEquals(DmCallButton.Return, DmCallButtons.of("dm-1", dm, "me", call))
        assertEquals(DmCallButton.Busy, DmCallButtons.of("dm-2", dm.copy(id = "dm-2"), "me", call))
        assertEquals(DmCallButton.Busy, DmCallButtons.of("dm-2", dm.copy(id = "dm-2"), "me", CallState.Incoming("dm-1", "c", "anna", null, 0L)))
    }
}
