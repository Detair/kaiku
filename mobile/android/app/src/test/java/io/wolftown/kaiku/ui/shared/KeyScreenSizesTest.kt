package io.wolftown.kaiku.ui.shared

import androidx.compose.ui.unit.dp
import org.junit.Assert.assertTrue
import org.junit.Test

/** Plan constraints (older users): approval, setup and keypad buttons ≥ 64 dp, other actions ≥ 56 dp. */
class KeyScreenSizesTest {
    @Test fun `every button on the key screens is at least 64 dp`() {
        // KeyPrimaryButton and KeySecondaryButton (Cancel, Share link, Back on setup and approval screens).
        assertTrue(KeyScreenSizes.primaryButton >= 64.dp)
        assertTrue(KeyScreenSizes.secondaryButton >= 64.dp)
    }

    @Test fun `other actions are at least 56 dp`() {
        assertTrue(KeyScreenSizes.otherAction >= 56.dp)
    }
}
