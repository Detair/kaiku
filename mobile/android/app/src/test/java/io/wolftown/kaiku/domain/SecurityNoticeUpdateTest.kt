package io.wolftown.kaiku.domain

import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.domain.model.AccountRef
import io.wolftown.kaiku.domain.model.Message
import io.wolftown.kaiku.domain.model.SecurityNotice
import io.wolftown.kaiku.domain.model.User
import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Test

class SecurityNoticeUpdateTest {
    private val kaiku = User("6b61696b-7500-4000-8000-000000000001", "kaiku", "Kaiku", isSystem = true)
    private fun notice(count: Int) = SecurityNotice("n1", AccountRef("u1", "alice"), "guessing", count = count)

    @Test fun `notice update replaces the card`() {
        val list = listOf(
            Message("m1", "c1", kaiku, "x", messageType = "security", securityNotice = notice(1)),
            Message("m2", "c1", kaiku, "y", messageType = "security", securityNotice = notice(1).copy(noticeId = "n2")),
        )
        val updated = applySecurityNoticeUpdate(list, ServerEvent.SecurityNoticeUpdated("c1", "m1", notice(5)))
        assertEquals(5, updated[0].securityNotice!!.count)
        assertSame(list[1], updated[1])
        // Unknown message: nothing changes.
        assertEquals(list, applySecurityNoticeUpdate(list, ServerEvent.SecurityNoticeUpdated("c1", "m9", notice(5))))
    }
}
