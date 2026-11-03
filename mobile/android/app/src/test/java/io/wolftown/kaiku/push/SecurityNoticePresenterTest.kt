package io.wolftown.kaiku.push

import io.wolftown.kaiku.R
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class SecurityNoticePresenterTest {

    private val notice = "dddddddd-dddd-4ddd-8ddd-dddddddddddd"
    private val me = "11111111-1111-4111-8111-111111111111"
    private val shown = mutableListOf<SecurityNotification>()
    private val presenter = SecurityNoticePresenter(sink = { shown += it }, ownUserId = { me })

    @Test fun `security notification is private and has no actions`() {
        presenter.present(SecurityWake(notice, "sign_in", "normal", 1, me, null))
        val n = shown.single()
        assertTrue(n.visibilityPrivate)
        assertTrue(n.actions.isEmpty())
        assertEquals(R.string.security_push_public, n.publicText)
        assertEquals(NotificationChannels.SECURITY, n.channelId)
        assertEquals(R.string.security_push_sign_in, n.text)
        assertEquals("kaiku://security", n.tapUri)
    }

    @Test fun `notification settings do not mute security notices`() {
        // No dependency on push preferences, quiet hours or the message coalescer: every wake is posted.
        repeat(3) { presenter.present(SecurityWake(notice, "guessing", "high", it + 1, me, null)) }
        assertEquals(3, shown.size)
    }

    @Test fun `updates of one notice reuse its notification id and carry the count`() {
        presenter.present(SecurityWake(notice, "guessing", "normal", 1, me, null))
        presenter.present(SecurityWake(notice, "guessing", "normal", 4, me, null))
        assertEquals(shown[0].notificationId, shown[1].notificationId)
        assertEquals(4, shown[1].count)
        presenter.present(SecurityWake("eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee", "guessing", "normal", 1, me, null))
        assertTrue(shown[2].notificationId != shown[0].notificationId)
    }

    @Test fun `the subject is your account or another account`() {
        presenter.present(SecurityWake(notice, "sign_in", "high", 1, me, null))
        assertEquals(R.string.security_subject_yours, shown.last().subject)
        presenter.present(SecurityWake(notice, "sign_in", "high", 1, "99999999-9999-4999-8999-999999999999", null))
        assertEquals(R.string.security_subject_other, shown.last().subject)
    }
}
