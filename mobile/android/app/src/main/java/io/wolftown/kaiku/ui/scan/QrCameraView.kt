package io.wolftown.kaiku.ui.scan

import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ExperimentalGetImage
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.google.mlkit.vision.barcode.BarcodeScanning
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.common.InputImage
import io.wolftown.kaiku.R
import io.wolftown.kaiku.ui.shared.KeyBody
import io.wolftown.kaiku.ui.shared.KeyPrimaryButton
import io.wolftown.kaiku.ui.shared.KeyScreen
import java.util.concurrent.Executors
import java.util.logging.Level
import java.util.logging.Logger

/**
 * The shared camera (L2-A7; stays in `src/main` for the simple app): CameraX
 * preview + ML Kit QR detection. Asks for CAMERA itself and shows its own
 * permission and camera errors. [onCode] gets every distinct raw code, on the
 * main thread; the caller decides what a code means.
 */
// ImageProxy.image is a CameraX opt-in API (androidx marker, checked by lint).
@androidx.annotation.OptIn(ExperimentalGetImage::class)
@Composable
fun QrCameraView(onCode: (String) -> Unit, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    val currentOnCode by rememberUpdatedState(onCode)
    var hasCameraPermission by remember {
        mutableStateOf(
            ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED,
        )
    }
    var cameraFailed by remember { mutableStateOf(false) }
    val logger = remember { Logger.getLogger("QrCameraView") }
    val barcodeScanner = remember { BarcodeScanning.getClient() }
    val analysisExecutor = remember { Executors.newSingleThreadExecutor() }
    val lastCode = remember { java.util.concurrent.atomic.AtomicReference<String?>(null) }

    DisposableEffect(Unit) {
        onDispose {
            barcodeScanner.close()
            analysisExecutor.shutdown()
        }
    }

    val permissionLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        hasCameraPermission = granted
    }
    LaunchedEffect(Unit) {
        if (!hasCameraPermission) permissionLauncher.launch(Manifest.permission.CAMERA)
    }

    when {
        cameraFailed -> KeyScreen(modifier) { KeyBody(stringResource(R.string.qr_scanner_camera_failed)) }
        !hasCameraPermission -> KeyScreen(modifier) {
            KeyBody(stringResource(R.string.qr_scanner_permission_required))
            KeyPrimaryButton(
                stringResource(R.string.qr_scanner_grant_permission),
                onClick = { permissionLauncher.launch(Manifest.permission.CAMERA) },
            )
        }
        else -> Box(modifier) {
            AndroidView(
                factory = { ctx ->
                    val previewView = PreviewView(ctx)
                    val providerFuture = ProcessCameraProvider.getInstance(ctx)
                    providerFuture.addListener({
                        val provider = providerFuture.get()
                        val preview = Preview.Builder().build().also { it.surfaceProvider = previewView.surfaceProvider }
                        val analysis = ImageAnalysis.Builder()
                            .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                            .build()
                        analysis.setAnalyzer(analysisExecutor) { imageProxy ->
                            val mediaImage = imageProxy.image
                            if (mediaImage == null) {
                                imageProxy.close()
                                return@setAnalyzer
                            }
                            val input = InputImage.fromMediaImage(mediaImage, imageProxy.imageInfo.rotationDegrees)
                            barcodeScanner.process(input)
                                .addOnSuccessListener { barcodes ->
                                    barcodes.firstOrNull { it.format == Barcode.FORMAT_QR_CODE }?.rawValue?.let { raw ->
                                        // The same code stays in view for many frames: report it once.
                                        if (lastCode.getAndSet(raw) != raw) currentOnCode(raw)
                                    }
                                }
                                .addOnFailureListener { e -> logger.log(Level.FINE, "Frame analysis failed: ${e.javaClass.simpleName}") }
                                .addOnCompleteListener { imageProxy.close() }
                        }
                        try {
                            provider.unbindAll()
                            provider.bindToLifecycle(lifecycleOwner, CameraSelector.DEFAULT_BACK_CAMERA, preview, analysis)
                        } catch (e: Exception) {
                            logger.warning("Camera bind failed: ${e.javaClass.simpleName}")
                            cameraFailed = true
                        }
                    }, ContextCompat.getMainExecutor(ctx))
                    previewView
                },
                modifier = Modifier.fillMaxSize(),
            )
        }
    }
}
