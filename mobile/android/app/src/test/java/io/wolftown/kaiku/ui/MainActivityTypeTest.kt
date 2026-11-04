package io.wolftown.kaiku.ui

import androidx.fragment.app.FragmentActivity
import org.junit.Assert.assertTrue
import org.junit.Test

/** BiometricPrompt needs a FragmentActivity host (L2-A2). */
class MainActivityTypeTest {
    @Test
    fun `MainActivity is a FragmentActivity`() {
        assertTrue(FragmentActivity::class.java.isAssignableFrom(MainActivity::class.java))
    }
}
