package io.wolftown.kaiku.ui.approval

import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.ClientClass
import io.wolftown.kaiku.approval.LoginContext
import io.wolftown.kaiku.approval.LoginVariant
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class LoginContextTextTest {

    private fun ctx(label: String = "Kaiku desktop on Windows", cls: ClientClass = ClientClass.DESKTOP,
                    variant: LoginVariant = LoginVariant.QR, place: String = "Hamburg, DE") =
        LoginContext("44444444-4444-4444-8444-444444444444", ByteArray(32), variant, cls, label, 1793707200,
            "Familie Müller", place, "alice")

    @Test fun `client line uses the label and falls back to the class`() {
        assertEquals(R.string.approval_client_labelled, clientLine(ctx()))
        assertEquals(R.string.approval_client_desktop, clientLine(ctx(label = "")))
        assertEquals(R.string.approval_client_web, clientLine(ctx(label = "", cls = ClientClass.WEB)))
        assertEquals(R.string.approval_client_android, clientLine(ctx(label = "", cls = ClientClass.ANDROID)))
    }

    @Test fun `variant line names a password or single sign-on`() {
        assertNull(variantLine(ctx()))
        assertEquals(R.string.approval_after_password, variantLine(ctx(variant = LoginVariant.PASSWORD)))
        assertEquals(R.string.approval_after_sso, variantLine(ctx(variant = LoginVariant.OIDC)))
    }

    @Test fun `empty place is not shown`() {
        assertTrue(showPlace(ctx()))
        assertFalse(showPlace(ctx(place = "")))
    }

    @Test fun `report result depends on the variant`() {
        assertEquals(R.string.approval_reported, reportedText(LoginVariant.QR))
        assertEquals(R.string.approval_reported_password, reportedText(LoginVariant.PASSWORD))
        assertEquals(R.string.approval_reported_oidc, reportedText(LoginVariant.OIDC))
    }
}
