package io.wolftown.kaiku.ui

import android.content.Intent
import android.os.Bundle
import android.widget.Toast
import io.wolftown.kaiku.BuildConfig
import io.wolftown.kaiku.R
import io.wolftown.kaiku.domain.normalizeServerAddress
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.navigation.compose.rememberNavController
import dagger.hilt.android.AndroidEntryPoint
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.ws.RealtimeConnectionManager
import io.wolftown.kaiku.navigation.DeepLinks
import io.wolftown.kaiku.navigation.PendingNavigation
import io.wolftown.kaiku.push.AndroidNotificationPresenter
import io.wolftown.kaiku.ui.auth.LoginViewModel
import io.wolftown.kaiku.ui.auth.OidcHandler
import io.wolftown.kaiku.ui.shared.KaikuTheme
import io.wolftown.kaiku.ui.shared.LocalServerBaseUrl
import javax.inject.Inject

@AndroidEntryPoint
class MainActivity : ComponentActivity() {

    @Inject
    lateinit var oidcHandler: OidcHandler

    @Inject
    lateinit var authState: AuthState

    @Inject
    lateinit var tokenStorage: TokenStorage

    @Inject
    lateinit var realtimeConnectionManager: RealtimeConnectionManager

    @Inject
    lateinit var pendingNavigation: PendingNavigation

    private val loginViewModel: LoginViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        authState.initialize(tokenStorage)
        // On recreation (rotation, theme change, process restore) getIntent() is the
        // original launch intent; handling it again would replay a consumed link.
        if (savedInstanceState == null) handleIntent(intent)

        val startDestination = resolveStartDestination()
        // After the start destination: reading the stored session may have found a damaged store.
        if (tokenStorage.consumeStorageResetNotice()) {
            Toast.makeText(this, R.string.auth_storage_reset, Toast.LENGTH_LONG).show()
        }
        val appVersion = packageManager.getPackageInfo(packageName, 0).versionName ?: ""

        setContent {
            KaikuApp(
                startDestination = startDestination,
                authState = authState,
                appVersion = appVersion,
                serverBaseUrl = tokenStorage::getServerUrl
            )
        }
    }

    override fun onStart() {
        super.onStart()
        // Reconnect right away when returning to the app instead of waiting out the backoff.
        realtimeConnectionManager.onAppForeground()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleIntent(intent)
    }

    /**
     * Checks whether the intent contains an OIDC callback deep link
     * (`kaiku://auth/callback`) and forwards it to the [LoginViewModel].
     */
    private fun handleIntent(intent: Intent?) {
        val uri = intent?.data ?: return
        if (oidcHandler.isOidcCallback(uri)) {
            loginViewModel.onOidcDeepLink(uri)
            return
        }
        DeepLinks.parseChannelId(uri.toString())?.let {
            // A notification tap names the account it was posted for; other links
            // belong to whoever is signed in now.
            val tag = intent.getStringExtra(AndroidNotificationPresenter.EXTRA_USER_ID)
                ?: authState.currentUserId.value
            pendingNavigation.post(it, tag)
            // Strip the consumed link so it cannot be handled twice.
            intent.data = null
            intent.removeExtra(AndroidNotificationPresenter.EXTRA_USER_ID)
        }
    }

    /**
     * Determines the initial navigation destination based on auth state:
     * - Logged in -> "home"
     * - Server URL saved but not logged in -> "login"
     * - No server URL -> "login" with the built-in default server saved; only when
     *   the build has no default does the person see "server_url"
     */
    private fun resolveStartDestination(): String {
        if (authState.isLoggedIn.value) return "home"
        if (tokenStorage.getServerUrl() == null) {
            val default = normalizeServerAddress(BuildConfig.DEFAULT_SERVER_URL).getOrNull()
                ?: return "server_url"
            tokenStorage.saveServerUrl(default)
        }
        return "login"
    }
}

/**
 * Root composable. [serverBaseUrl] reads the configured server URL; it is
 * re-read whenever the login state changes (the URL is saved before login)
 * and provided as [LocalServerBaseUrl] so screens can resolve the relative
 * image URLs the server sends.
 */
@Composable
fun KaikuApp(
    authState: AuthState,
    startDestination: String = "server_url",
    appVersion: String = "",
    serverBaseUrl: () -> String? = { null }
) {
    val isLoggedIn by authState.isLoggedIn.collectAsState()
    val baseUrl = remember(isLoggedIn) { serverBaseUrl() }

    CompositionLocalProvider(LocalServerBaseUrl provides baseUrl) {
        KaikuTheme(darkTheme = true) {
            Surface(
                modifier = Modifier.fillMaxSize(),
                color = MaterialTheme.colorScheme.background,
            ) {
                val navController = rememberNavController()
                KaikuNavGraph(
                    navController = navController,
                    startDestination = startDestination,
                    authState = authState,
                    appVersion = appVersion
                )
            }
        }
    }
}
