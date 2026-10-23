package io.wolftown.kaiku.ui.shared

import androidx.compose.runtime.Composable
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.staticCompositionLocalOf
import io.wolftown.kaiku.data.api.ServerUrls

/** The configured server URL, used to resolve relative image URLs in the UI. */
val LocalServerBaseUrl = staticCompositionLocalOf<String?> { null }

/** Resolves a server-sent image URL against [LocalServerBaseUrl]; see [ServerUrls.resolve]. */
@Composable
@ReadOnlyComposable
fun resolveServerUrl(url: String?): String? = ServerUrls.resolve(LocalServerBaseUrl.current, url)
