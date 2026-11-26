package io.wolftown.kaiku.ui.auth

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.auth.KeyStorageProblem
import io.wolftown.kaiku.data.auth.keyStorageTexts
import io.wolftown.kaiku.ui.shared.KaikuTheme
import io.wolftown.kaiku.ui.shared.KeyBody
import io.wolftown.kaiku.ui.shared.KeyNotice
import io.wolftown.kaiku.ui.shared.KeyPrimaryButton
import io.wolftown.kaiku.ui.shared.KeyScreen
import io.wolftown.kaiku.ui.shared.KeyTitle

/**
 * Shown instead of the app while what the phone has saved about its key cannot be read, or a write
 * of it did not reach storage ([problem]): the app decides nothing then (it neither treats the
 * phone as having no key nor opens a stored sign-in).
 */
@Composable
fun KeyStorageUnreadable(problem: KeyStorageProblem, onRetry: () -> Unit) {
    val texts = keyStorageTexts(problem)
    KaikuTheme(darkTheme = true) {
        Surface(modifier = Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
            KeyScreen {
                KeyTitle(stringResource(R.string.app_name))
                KeyNotice(stringResource(texts.notice), warning = true)
                KeyPrimaryButton(stringResource(texts.button), onClick = onRetry)
                texts.lastResort?.let { KeyBody(stringResource(it), secondary = true) }
            }
        }
    }
}
