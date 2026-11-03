package io.wolftown.kaiku.ui.scan

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.domain.ScanMode
import io.wolftown.kaiku.domain.ScanPayload
import io.wolftown.kaiku.ui.shared.KeyBody
import io.wolftown.kaiku.ui.shared.KeyNotice
import io.wolftown.kaiku.ui.shared.KeyPrimaryButton
import io.wolftown.kaiku.ui.shared.KeyScreen
import io.wolftown.kaiku.ui.shared.KeySecondaryButton

/**
 * The one scanner (L2-A7): sign-in codes from a PC and setup codes. Another
 * server's code, a code that needs an update, or a sign-in code this phone may
 * not confirm is refused here, before any request.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ScanScreen(
    mode: ScanMode,
    onLogin: (ScanPayload.Login) -> Unit,
    onSetup: (ScanPayload.Setup) -> Unit,
    onBack: () -> Unit,
    viewModel: ScanViewModel = hiltViewModel(),
) {
    viewModel.setMode(mode)
    val ui by viewModel.ui.collectAsState()

    LaunchedEffect(ui) {
        when (val u = ui) {
            is ScanUi.OpenLogin -> {
                viewModel.rescan()
                onLogin(u.payload)
            }
            is ScanUi.OpenSetup -> {
                viewModel.rescan()
                onSetup(u.payload)
            }
            else -> Unit
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(if (mode == ScanMode.SETUP_ONLY) R.string.scan_setup_title else R.string.scan_title)) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.common_back))
                    }
                },
            )
        },
    ) { padding ->
        when (val u = ui) {
            is ScanUi.Refused -> KeyScreen(Modifier.padding(padding)) {
                KeyNotice(stringResource(u.msg), warning = true)
                KeyPrimaryButton(stringResource(R.string.scan_again), onClick = viewModel::rescan)
                KeySecondaryButton(stringResource(R.string.common_back), onClick = onBack)
            }
            else -> Box(Modifier.fillMaxSize().padding(padding)) {
                QrCameraView(onCode = viewModel::onCode, modifier = Modifier.fillMaxSize())
                Column(
                    Modifier
                        .align(Alignment.BottomCenter)
                        .fillMaxWidth()
                        .background(MaterialTheme.colorScheme.surface)
                        .padding(16.dp),
                ) {
                    KeyBody(stringResource(if (mode == ScanMode.SETUP_ONLY) R.string.scan_setup_hint else R.string.scan_hint))
                }
            }
        }
    }
}
