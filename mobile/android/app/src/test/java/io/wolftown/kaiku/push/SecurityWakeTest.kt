package io.wolftown.kaiku.push

import io.mockk.mockk
import io.mockk.verify
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class SecurityWakeTest {

    private val notice = "dddddddd-dddd-4ddd-8ddd-dddddddddddd"
    private val subject = "11111111-1111-4111-8111-111111111111"

    @Test fun `fcm and unifiedpush security wakes parse`() {
        val fcm = SecurityWakeParser.fromFcmData(
            mapOf("t" to "security", "notice_id" to notice, "event_type" to "sign_in", "severity" to "high",
                "count" to "3", "subject_id" to subject, "actor_id" to "22222222-2222-4222-8222-222222222222"),
        )
        assertEquals(SecurityWake(notice, "sign_in", "high", 3, subject, "22222222-2222-4222-8222-222222222222"), fcm)
        val up = SecurityWakeParser.fromUnifiedPushBody(
            """{"t":"security","notice_id":"$notice","event_type":"guessing","severity":"normal","count":2,"subject_id":"$subject"}""".toByteArray(),
        )
        assertEquals(SecurityWake(notice, "guessing", "normal", 2, subject, null), up)
        // Not a security wake, or broken ids: nothing.
        assertNull(SecurityWakeParser.fromFcmData(mapOf("t" to "dm", "channel_id" to notice)))
        assertNull(SecurityWakeParser.fromFcmData(mapOf("t" to "security", "notice_id" to "x", "event_type" to "sign_in", "subject_id" to subject)))
        assertEquals(WakeKind.SECURITY, WakeKind.fromWire("security"))
    }

    @Test fun `security wake is shown without a session and fetches nothing`() {
        val enqueuer: WakeWorkEnqueuer = mockk(relaxed = true)
        val shown = mutableListOf<SecurityNotification>()
        val presenter = SecurityNoticePresenter(sink = { shown += it }, ownUserId = { null })
        val scheduler = WakeScheduler(enqueuer, mockk(relaxed = true), mockk(relaxed = true), presenter)
        scheduler.onSecurityWake(SecurityWake(notice, "sign_in", "high", 1, subject, null))
        assertEquals(1, shown.size)
        verify(exactly = 0) { enqueuer.enqueue(any()) }
    }

    @Test fun `unifiedpush message hands a security wake to its handler`() {
        var got: SecurityWake? = null
        val body = """{"t":"security","notice_id":"$notice","event_type":"sign_in","severity":"normal","count":1,"subject_id":"$subject"}""".toByteArray()
        assertEquals(true, handleUnifiedPushMessage(body, onWake = { error("not a channel wake") }, onSecurity = { got = it }))
        assertEquals(notice, got!!.noticeId)
    }
}
