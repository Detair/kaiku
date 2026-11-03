package io.wolftown.kaiku.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import io.wolftown.kaiku.data.keydevice.KeyDeviceState
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.ui.auth.ConfirmItsYouScreen
import io.wolftown.kaiku.domain.ScanMode
import io.wolftown.kaiku.navigation.ScanHandoff
import io.wolftown.kaiku.ui.scan.ScanScreen
import io.wolftown.kaiku.ui.approval.LoginApprovalScreen
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import io.wolftown.kaiku.navigation.PendingNavigationRules
import io.wolftown.kaiku.ui.auth.ChangePasswordScreen
import io.wolftown.kaiku.ui.auth.LoginScreen
import io.wolftown.kaiku.ui.auth.RegisterScreen
import io.wolftown.kaiku.ui.auth.ServerUrlScreen
import io.wolftown.kaiku.ui.channel.TextChannelScreen
import io.wolftown.kaiku.ui.dm.DmListScreen
import io.wolftown.kaiku.ui.home.HomeScreen
import io.wolftown.kaiku.ui.settings.SettingsScreen
import io.wolftown.kaiku.ui.voice.VoiceChannelScreen
import io.wolftown.kaiku.ui.voice.VoiceOverlay
import io.wolftown.kaiku.ui.voice.VoiceOverlayViewModel
import io.wolftown.kaiku.ui.voice.VOICE_ROUTE
import io.wolftown.kaiku.ui.voice.shouldShowVoiceOverlay

@Composable
fun KaikuNavGraph(
    navController: NavHostController,
    startDestination: String,
    authState: AuthState,
    appVersion: String = "",
    keyDeviceState: StateFlow<KeyDeviceState> = MutableStateFlow(KeyDeviceState.None),
    pendingRoute: StateFlow<String?> = MutableStateFlow(null),
    onPendingRouteConsumed: () -> Unit = {},
    pendingNavigationViewModel: PendingNavigationViewModel = hiltViewModel()
) {
    val keyState by keyDeviceState.collectAsState()
    val nextRoute by pendingRoute.collectAsState()
    val pendingNavigation = pendingNavigationViewModel.pendingNavigation
    val pendingChannelId by pendingNavigation.channelId.collectAsState()
    val currentUserId by authState.currentUserId.collectAsState()
    val isLoggedIn by authState.isLoggedIn.collectAsState()
    val navBackStackEntry by navController.currentBackStackEntryAsState()
    val currentRoute = navBackStackEntry?.destination?.route

    // Auth guard: redirect to login when logged out from an authenticated screen
    val authRoutes = setOf(
        "server_url", "login", "register", "change_password", SCAN_SETUP_ROUTE,
        CONFIRM_ROUTE,
    )
    LaunchedEffect(isLoggedIn) {
        if (!isLoggedIn && currentRoute != null && currentRoute !in authRoutes) {
            navController.navigate("login") {
                popUpTo(0) { inclusive = true }
            }
        }
    }

    // A lapsed key device never sees the password form: it confirms with its screen lock (L2-A11).
    LaunchedEffect(isLoggedIn, keyState, currentRoute) {
        if (!isLoggedIn && keyState is KeyDeviceState.Lapsed && currentRoute == "login") {
            navController.navigate(CONFIRM_ROUTE) { popUpTo(0) { inclusive = true } }
        }
    }

    // The renewal prompt (B.4) and other routes the activity asks for.
    LaunchedEffect(nextRoute, isLoggedIn, currentRoute) {
        val route = nextRoute ?: return@LaunchedEffect
        if (isLoggedIn && currentRoute != null && currentRoute !in authRoutes) {
            onPendingRouteConsumed()
            navController.navigate(route) { launchSingleTop = true }
        }
    }

    // Any transition into logged-out clears a pending link, whatever the route.
    var wasLoggedIn by remember { mutableStateOf(isLoggedIn) }
    LaunchedEffect(isLoggedIn) {
        if (!isLoggedIn && wasLoggedIn) pendingNavigation.clear()
        wasLoggedIn = isLoggedIn
    }

    // Open a channel from a deep link / notification tap, only while signed in.
    // The "home" entry must be on the stack first (not during login screens).
    LaunchedEffect(isLoggedIn, pendingChannelId, currentRoute, currentUserId) {
        val id = pendingChannelId
        val tag = pendingNavigation.userTag
        if (id != null && PendingNavigationRules.shouldDrop(isLoggedIn, tag, currentUserId)) {
            pendingNavigation.clear()
        } else if (id != null && PendingNavigationRules.shouldConsume(
                isLoggedIn, tag, currentUserId, currentRoute, authRoutes
            )
        ) {
            pendingNavigation.consume()
            navController.navigate("channel/$id") {
                popUpTo("home")
                launchSingleTop = true
            }
        }
    }

    // Voice overlay state: a call keeps running when the user leaves the voice
    // screen; the bar shows on every other screen and leads back to it.
    val voiceOverlayViewModel: VoiceOverlayViewModel = hiltViewModel()
    val voiceChannelId by voiceOverlayViewModel.currentChannelId.collectAsState()
    val voiceChannelName by voiceOverlayViewModel.channelName.collectAsState()
    val voiceIsMuted by voiceOverlayViewModel.isMuted.collectAsState()
    val showOverlay = shouldShowVoiceOverlay(voiceChannelId, currentRoute)

    Column(modifier = Modifier.fillMaxSize()) {
        Box(modifier = Modifier.weight(1f)) {
            NavHost(navController = navController, startDestination = startDestination) {
                composable("server_url") {
                    ServerUrlScreen(
                        onConnectSuccess = {
                            navController.navigate("login") {
                                popUpTo("server_url") { inclusive = true }
                            }
                        },
                        onScanQrCode = {
                            navController.navigate(SCAN_SETUP_ROUTE)
                        },
                        onBack = {
                            navController.navigate("login") {
                                popUpTo("server_url") { inclusive = true }
                            }
                        }
                    )
                }

                composable("login") {
                    LoginScreen(
                        onNavigateToRegister = {
                            navController.navigate("register")
                        },
                        onLoginSuccess = {
                            navController.navigate("home") {
                                popUpTo("login") { inclusive = true }
                            }
                        },
                        onPasswordChangeRequired = {
                            navController.navigate("change_password")
                        },
                        onChangeServer = {
                            // Replace login so no screen below keeps the old server's state.
                            navController.navigate("server_url") {
                                popUpTo("login") { inclusive = true }
                            }
                        }
                    )
                }

                composable("change_password") {
                    ChangePasswordScreen(
                        onPasswordChanged = {
                            navController.navigate("home") {
                                popUpTo(0) { inclusive = true }
                            }
                        },
                        onCancel = { navController.popBackStack() }
                    )
                }

                composable("register") {
                    RegisterScreen(
                        onNavigateToLogin = {
                            navController.popBackStack()
                        },
                        onRegisterSuccess = {
                            navController.navigate("home") {
                                popUpTo("register") { inclusive = true }
                            }
                        }
                    )
                }

                composable("home") {
                    HomeScreen(
                        onNavigateToTextChannel = { channelId ->
                            navController.navigate("channel/$channelId")
                        },
                        onNavigateToVoiceChannel = { channelId ->
                            navController.navigate("voice/$channelId")
                        },
                        onNavigateToSettings = {
                            navController.navigate("settings")
                        },
                        onNavigateToDms = {
                            navController.navigate("dms")
                        },
                        onNavigateToScan = { navController.navigate(SCAN_ROUTE) },
                    )
                }

                composable("dms") {
                    DmListScreen(
                        onOpenDm = { channelId ->
                            navController.navigate("channel/$channelId")
                        },
                        onNavigateBack = { navController.popBackStack() },
                        onScan = { navController.navigate(SCAN_ROUTE) },
                    )
                }

                composable("channel/{channelId}") {
                    TextChannelScreen(
                        currentUserId = currentUserId ?: "",
                        onNavigateBack = { navController.popBackStack() }
                    )
                }

                composable(VOICE_ROUTE) {
                    VoiceChannelScreen(
                        onNavigateBack = { navController.popBackStack() }
                    )
                }

                composable("settings") {
                    SettingsScreen(
                        appVersion = appVersion,
                        onNavigateBack = { navController.popBackStack() },
                        onLogout = {
                            navController.navigate("login") {
                                popUpTo(0) { inclusive = true }
                            }
                        }
                    )
                }

                composable(CONFIRM_ROUTE) {
                    ConfirmItsYouScreen(
                        onSignedIn = {
                            navController.navigate("home") { popUpTo(0) { inclusive = true } }
                        },
                        onSignedOut = {
                            navController.navigate("login") { popUpTo(0) { inclusive = true } }
                        },
                    )
                }

                composable(CONFIRM_RENEW_ROUTE) {
                    ConfirmItsYouScreen(
                        renewal = true,
                        onSignedIn = { navController.popBackStack() },
                        onLater = { navController.popBackStack() },
                        onSignedOut = {
                            navController.navigate("login") { popUpTo(0) { inclusive = true } }
                        },
                    )
                }

                composable(SCAN_ROUTE) {
                    ScanScreen(
                        mode = ScanMode.SIGN_IN_OR_SETUP,
                        onLogin = { payload ->
                            ScanHandoff.putLogin(payload)
                            navController.navigate(APPROVE_LOGIN_ROUTE) { popUpTo(SCAN_ROUTE) { inclusive = true } }
                        },
                        onSetup = { payload ->
                            ScanHandoff.putSetup(payload)
                            navController.navigate(SETUP_FLOW_ROUTE) { popUpTo(SCAN_ROUTE) { inclusive = true } }
                        },
                        onBack = { navController.popBackStack() },
                    )
                }

                composable(APPROVE_LOGIN_ROUTE) {
                    LoginApprovalScreen(
                        takePayload = ScanHandoff::takeLogin,
                        onRescan = {
                            navController.navigate(SCAN_ROUTE) { popUpTo(APPROVE_LOGIN_ROUTE) { inclusive = true } }
                        },
                        onDone = { navController.popBackStack() },
                    )
                }

                composable(SCAN_SETUP_ROUTE) {
                    ScanScreen(
                        mode = ScanMode.SETUP_ONLY,
                        onLogin = {},
                        onSetup = { payload ->
                            ScanHandoff.putSetup(payload)
                            navController.navigate(SETUP_FLOW_ROUTE) { popUpTo(SCAN_SETUP_ROUTE) { inclusive = true } }
                        },
                        onBack = { navController.popBackStack() },
                    )
                }
            }
        }

        // Voice overlay bar — visible on non-voice screens when in a voice channel
        if (showOverlay) {
            VoiceOverlay(
                channelName = voiceChannelName,
                isMuted = voiceIsMuted,
                onMuteToggle = { voiceOverlayViewModel.onToggleMute() },
                onTap = {
                    voiceChannelId?.let { channelId ->
                        navController.navigate("voice/$channelId") { launchSingleTop = true }
                    }
                },
                onDisconnect = { voiceOverlayViewModel.onDisconnect() }
            )
        }
    }
}

/** L2-A11: the lapsed key device's sign-in screen. */
const val CONFIRM_ROUTE = "confirm_its_you"

/** The B.4 renewal prompt over a running session. */
const val CONFIRM_RENEW_ROUTE = "confirm_its_you_renew"

/** The one scanner in the full app (sign-in and setup codes). */
const val SCAN_ROUTE = "scan"

/** The scanner for setup codes only (before sign-in). */
const val SCAN_SETUP_ROUTE = "scan_setup"

/** Confirming a PC sign-in after a scan (Task B8). */
const val APPROVE_LOGIN_ROUTE = "approve_login"

/** Setting up this phone from a setup code or link (Task B9). */
const val SETUP_FLOW_ROUTE = "setup_flow"
