package io.wolftown.kaiku.ui.scan

import io.wolftown.kaiku.R
import io.wolftown.kaiku.domain.ScanPayload
import io.wolftown.kaiku.domain.ScanMode
import org.junit.Assert.assertEquals
import org.junit.Test

class ScanViewModelTest {

    private val token = "oKGio6SlpqeoqaqrrK2ur7CxsrO0tba3uLm6u7y9vr8"
    private val loginCode = "kaiku://login/v1?o=https%3A%2F%2Fchat.example.com&t=$token"
    private val setupCode = "https://chat.example.com/setup#v=1&t=$token"
    private var pinned: String? = "https://chat.example.com"
    private var mayApprove: Boolean? = true
    private var isKey = true

    private fun vm(mode: ScanMode = ScanMode.SIGN_IN_OR_SETUP) = ScanViewModel(
        pinnedOrigin = { pinned }, mayApprove = { mayApprove }, isKeyDevice = { isKey },
    ).also { it.setMode(mode) }

    @Test fun `own login code opens the approval`() {
        val v = vm()
        v.onCode(loginCode)
        assertEquals(ScanUi.OpenLogin(ScanPayload.Login("https://chat.example.com", token)), v.ui.value)
    }

    @Test fun `other origin is refused before any request`() {
        val v = vm()
        v.onCode("kaiku://login/v1?o=https%3A%2F%2Fevil.example.com&t=$token")
        assertEquals(ScanUi.Refused(R.string.scan_other_server), v.ui.value)
        v.rescan()
        v.onCode("https://evil.example.com/setup#v=1&t=$token")
        assertEquals(ScanUi.Refused(R.string.scan_other_server), v.ui.value)
    }

    @Test fun `unconfigured app accepts a setup code of any server but no login code`() {
        pinned = null; isKey = false; mayApprove = null
        val v = vm()
        v.onCode("https://other.example.org/setup#v=1&t=$token")
        assertEquals(ScanUi.OpenSetup(ScanPayload.Setup("https://other.example.org", token)), v.ui.value)
        v.rescan()
        v.onCode(loginCode)
        assertEquals(ScanUi.Refused(R.string.scan_not_a_key), v.ui.value)
    }

    @Test fun `setup only mode refuses a login code`() {
        val v = vm(ScanMode.SETUP_ONLY)
        v.onCode(loginCode)
        assertEquals(ScanUi.Refused(R.string.scan_sign_in_code_in_setup), v.ui.value)
    }

    @Test fun `may approve off refuses a login code without a request`() {
        mayApprove = false
        val v = vm()
        v.onCode(loginCode)
        assertEquals(ScanUi.Refused(R.string.scan_may_approve_off), v.ui.value)
    }

    @Test fun `codes that are not kaiku or need an update say so`() {
        val v = vm()
        v.onCode("hello")
        assertEquals(ScanUi.Refused(R.string.scan_not_kaiku), v.ui.value)
        v.rescan()
        v.onCode("kaiku://login/v2?o=x&t=y")
        assertEquals(ScanUi.Refused(R.string.scan_needs_update), v.ui.value)
        v.rescan()
        v.onCode("kaiku://qr/login?server=x&token=y")
        assertEquals(ScanUi.Refused(R.string.scan_old_kaiku_code), v.ui.value)
    }

    @Test fun `a second code is ignored until rescan`() {
        val v = vm()
        v.onCode("hello")
        v.onCode(loginCode)
        assertEquals(ScanUi.Refused(R.string.scan_not_kaiku), v.ui.value)
    }

    @Test fun `a key phone refuses a setup code`() {
        isKey = true
        val v = vm()
        v.onCode(setupCode)
        assertEquals(ScanUi.Refused(R.string.setup_already_key), v.ui.value)
        isKey = false
        v.rescan()
        v.onCode(setupCode)
        assertEquals(ScanUi.OpenSetup(ScanPayload.Setup("https://chat.example.com", token)), v.ui.value)
    }
}
