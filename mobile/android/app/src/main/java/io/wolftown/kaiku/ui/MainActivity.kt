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
import androidx.compose.ui.Modifier
import androidx.navigation.compose.rememberNavController
import dagger.hilt.android.AndroidEntryPoint
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.ui.auth.LoginViewModel
import io.wolftown.kaiku.ui.auth.OidcHandler
import io.wolftown.kaiku.ui.shared.KaikuTheme
import javax.inject.Inject

@AndroidEntryPoint
class MainActivity : ComponentActivity() {

    @Inject
    lateinit var oidcHandler: OidcHandler

    @Inject
    lateinit var authState: AuthState

    @Inject
    lateinit var tokenStorage: TokenStorage

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
                appVersion = appVersion
            )
        }
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
        }
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

@Composable
fun KaikuApp(
    authState: AuthState,
    startDestination: String = "server_url",
    appVersion: String = ""
) {
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
