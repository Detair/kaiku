package io.wolftown.kaiku.approval

import org.junit.Assert.assertEquals
import org.junit.Test

class UvPolicyTest {

    @Test fun `api30 secure uses per use with biometric or credential`() {
        val d = UvPolicy.choose(apiLevel = 30, deviceSecure = true, strongBiometric = false)
        assertEquals(UvDecision.Use(UvClass.PER_USE, UvAuthenticators.BIOMETRIC_STRONG or UvAuthenticators.DEVICE_CREDENTIAL, api30 = true), d)
        assertEquals(d, UvPolicy.choose(apiLevel = 35, deviceSecure = true, strongBiometric = true))
    }

    @Test fun `api29 strong biometric uses per use biometric`() {
        assertEquals(
            UvDecision.Use(UvClass.PER_USE, UvAuthenticators.BIOMETRIC_STRONG, api30 = false),
            UvPolicy.choose(apiLevel = 29, deviceSecure = true, strongBiometric = true),
        )
        assertEquals(
            UvDecision.Use(UvClass.PER_USE, UvAuthenticators.BIOMETRIC_STRONG, api30 = false),
            UvPolicy.choose(apiLevel = 26, deviceSecure = true, strongBiometric = true),
        )
    }

    @Test fun `api29 pin only uses the ten second window`() {
        assertEquals(
            UvDecision.Use(UvClass.WINDOW_10S, UvAuthenticators.DEVICE_CREDENTIAL, api30 = false),
            UvPolicy.choose(apiLevel = 29, deviceSecure = true, strongBiometric = false),
        )
    }

    @Test fun `insecure device is unsuitable on every api`() {
        for (api in 26..35) {
            for (bio in listOf(true, false)) {
                assertEquals(
                    UvDecision.Unsuitable(UnsuitableReason.NO_SCREEN_LOCK),
                    UvPolicy.choose(apiLevel = api, deviceSecure = false, strongBiometric = bio),
                )
            }
        }
    }
}
