package io.wolftown.kaiku.ui

import android.content.Intent
import android.os.Bundle
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
        handleIntent(intent)

        val startDestination = resolveStartDestination()
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
        DeepLinks.parseChannelId(uri.toString())?.let(pendingNavigation::post)
    }

    /**
     * Determines the initial navigation destination based on auth state:
     * - Logged in -> "home"
     * - Server URL saved but not logged in -> "login"
     * - No server URL -> "server_url"
     */
    private fun resolveStartDestination(): String {
        return when {
            authState.isLoggedIn.value -> "home"
            tokenStorage.getServerUrl() != null -> "login"
            else -> "server_url"
        }
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
