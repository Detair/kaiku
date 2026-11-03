package io.wolftown.kaiku.ui.approval

import androidx.annotation.StringRes
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.ClientClass
import io.wolftown.kaiku.approval.LoginContext
import io.wolftown.kaiku.approval.LoginVariant
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale

/**
 * The client line, shown first: the signed `client_label` ("Kaiku desktop on
 * Windows", `%1$s`), or the client kind from `client_class` when the label is empty.
 */
@StringRes
fun clientLine(ctx: LoginContext): Int = when {
    ctx.clientLabel.isNotEmpty() -> R.string.approval_client_labelled
    ctx.clientClass == ClientClass.DESKTOP -> R.string.approval_client_desktop
    ctx.clientClass == ClientClass.WEB -> R.string.approval_client_web
    else -> R.string.approval_client_android
}

/** F3: a confirmation after a password or a single sign-on says so. */
@StringRes
fun variantLine(ctx: LoginContext): Int? = when (ctx.variant) {
    LoginVariant.QR -> null
    LoginVariant.PASSWORD -> R.string.approval_after_password
    LoginVariant.OIDC -> R.string.approval_after_sso
}

/** The approximate place is shown only when the server has one. */
fun showPlace(ctx: LoginContext): Boolean = ctx.place.isNotEmpty()

/** After "I didn't start this": what happened and what to do next (F3). */
@StringRes
fun reportedText(variant: LoginVariant): Int = when (variant) {
    LoginVariant.QR -> R.string.approval_reported
    LoginVariant.PASSWORD -> R.string.approval_reported_password
    LoginVariant.OIDC -> R.string.approval_reported_oidc
}

/** The attempt's creation time (signed Unix seconds) in the phone's zone and locale. */
fun createdAtText(ctx: LoginContext, zone: ZoneId = ZoneId.systemDefault(), locale: Locale = Locale.getDefault()): String =
    DateTimeFormatter.ofLocalizedDateTime(FormatStyle.MEDIUM, FormatStyle.SHORT)
        .withLocale(locale)
        .format(Instant.ofEpochSecond(ctx.createdAt).atZone(zone))
