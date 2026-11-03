package io.wolftown.kaiku.ui.shared

import com.google.zxing.BarcodeFormat
import com.google.zxing.EncodeHintType
import com.google.zxing.common.BitMatrix
import com.google.zxing.qrcode.QRCodeWriter
import com.google.zxing.qrcode.decoder.ErrorCorrectionLevel

/** Quiet zone around every QR code Kaiku draws, in modules (ISO/IEC 18004 asks for 4). */
const val QR_QUIET_ZONE_MODULES = 4

/**
 * Encodes [payload] as a QR code: error correction M, a four-module quiet zone,
 * one matrix cell per module (the caller scales it). Pure JVM, so it is tested
 * without Android.
 */
fun qrMatrix(payload: String): BitMatrix =
    QRCodeWriter().encode(
        payload,
        BarcodeFormat.QR_CODE,
        0,
        0,
        mapOf(
            EncodeHintType.ERROR_CORRECTION to ErrorCorrectionLevel.M,
            EncodeHintType.MARGIN to QR_QUIET_ZONE_MODULES,
            EncodeHintType.CHARACTER_SET to "UTF-8",
        ),
    )
