package io.wolftown.kaiku.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
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
import io.wolftown.kaiku.ui.approval.AddPhoneScreen
import io.wolftown.kaiku.ui.approval.IssuerMode
import io.wolftown.kaiku.ui.settings.security.SecuritySettingsScreen
import io.wolftown.kaiku.ui.security.OpenRequestPrompt
import io.wolftown.kaiku.ui.security.OpenRequestRules
import io.wolftown.kaiku.ui.security.OpenRequestViewModel
import io.wolftown.kaiku.ui.security.SecurityLogScreen
import io.wolftown.kaiku.ui.admin.AdminEntryViewModel
import io.wolftown.kaiku.ui.admin.AdminSetupPhoneScreen
import io.wolftown.kaiku.ui.admin.AdminUserScreen
import io.wolftown.kaiku.ui.admin.AdminUsersScreen
import io.wolftown.kaiku.navigation.PendingSetupLinks
import io.wolftown.kaiku.ui.auth.setup.SetupEntry
import io.wolftown.kaiku.ui.auth.setup.SetupFlowScreen
import io.wolftown.kaiku.ui.auth.setup.SetupPendingScreen
import io.wolftown.kaiku.ui.auth.setup.SetupScanScreen
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
        CONFIRM_ROUTE, SETUP_FLOW_ROUTE, SETUP_PENDING_ROUTE,
    )
    LaunchedEffect(isLoggedIn) {
        if (!isLoggedIn && currentRoute != null && currentRoute !in authRoutes) {
            navController.navigate("login") {
                popUpTo(0) { inclusive = true }
            }
        }
    }

    // A key device never sees the password form: a lapsed one confirms with its screen lock
    // (L2-A11), a pending one keeps showing when it will be ready (L2-A13).
    LaunchedEffect(isLoggedIn, keyState, currentRoute) {
        if (!isLoggedIn && currentRoute == "login") {
            when (keyState) {
                is KeyDeviceState.Lapsed -> navController.navigate(CONFIRM_ROUTE) { popUpTo(0) { inclusive = true } }
                is KeyDeviceState.Pending -> navController.navigate(SETUP_PENDING_ROUTE) { popUpTo(0) { inclusive = true } }
                else -> Unit
            }
        }
    }

    // A setup code from a link, the scanner or this app's own registration (L2-A15).
    val pendingSetup by PendingSetupLinks.pending.collectAsState()
    LaunchedEffect(pendingSetup, currentRoute) {
        if (pendingSetup != null && currentRoute != null && currentRoute != SETUP_FLOW_ROUTE) {
            navController.navigate(SETUP_FLOW_ROUTE) { launchSingleTop = true }
        }
    }

    // The renewal prompt (B.4) and other routes the activity asks for.
    LaunchedEffect(nextRoute, isLoggedIn, currentRoute) {
        val route = nextRoute ?: return@LaunchedEffect
        // Never on top of a running ceremony (approving, scanning, adding a phone): it waits for it.
        if (isLoggedIn && OpenRequestRules.showOver(currentRoute, authRoutes, CEREMONY_ROUTES, OPEN_REQUEST_ROUTE)) {
            onPendingRouteConsumed()
            navController.navigate(route) { launchSingleTop = true }
        }
    }

    // L2-A10: a waiting or ready account request is shown full screen. It is read again at
    // every foreground and every account change, for that account only (signed out: cleared),
    // and shown over any signed-in screen except a running ceremony.
    val openRequestViewModel: OpenRequestViewModel = hiltViewModel()
    val openRequest by openRequestViewModel.pending.collectAsState()
    val lifecycleOwner = LocalLifecycleOwner.current
    val openRequestAccount = if (isLoggedIn) currentUserId else null
    DisposableEffect(lifecycleOwner, openRequestAccount) {
        openRequestViewModel.refresh(openRequestAccount)
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_START) openRequestViewModel.refresh(openRequestAccount)
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }
    LaunchedEffect(openRequest, currentRoute, isLoggedIn) {
        if (openRequest != null && isLoggedIn &&
            OpenRequestRules.showOver(currentRoute, authRoutes, CEREMONY_ROUTES, OPEN_REQUEST_ROUTE)
        ) {
            navController.navigate(OPEN_REQUEST_ROUTE) { launchSingleTop = true }
        }
    }

    // Any transition into logged-out clears a pending link, whatever the route.
    var wasLoggedIn by remember { mutableStateOf(isLoggedIn) }
    LaunchedEffect(isLoggedIn) {
        if (!isLoggedIn && wasLoggedIn) {
            pendingNavigation.clear()
            // What the signed-in phone scanned or was handed is not the next person's.
            PendingSetupLinks.clear()
            ScanHandoff.clear()
        }
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
                        },
                        onSetUpPhone = { navController.navigate(SCAN_SETUP_ROUTE) },
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
                        onRegistered = { entry -> PendingSetupLinks.post(entry) }
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
                    val adminEntry: AdminEntryViewModel = hiltViewModel()
                    val isAdmin by adminEntry.isAdmin.collectAsState()
                    SettingsScreen(
                        onOpenAdmin = if (isAdmin) ({ navController.navigate(ADMIN_ROUTE) }) else null,
                        appVersion = appVersion,
                        onNavigateBack = { navController.popBackStack() },
                        onLogout = {
                            navController.navigate("login") {
                                popUpTo(0) { inclusive = true }
                            }
                        },
                        onOpenSecurity = { navController.navigate(SECURITY_SETTINGS_ROUTE) },
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
                            PendingSetupLinks.post(SetupEntry(payload.origin, payload.tokenB64u, SetupEntry.Source.SCAN))
                            // The scanner (and its camera) does not stay underneath the setup.
                            navController.navigate(SETUP_FLOW_ROUTE) {
                                popUpTo(SCAN_ROUTE) { inclusive = true }
                                launchSingleTop = true
                            }
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

                composable(SECURITY_SETTINGS_ROUTE) {
                    SecuritySettingsScreen(
                        onBack = { navController.popBackStack() },
                        onAddPhone = { navController.navigate(ADD_PHONE_ROUTE) },
                        onSignedOut = { navController.navigate("login") { popUpTo(0) { inclusive = true } } },
                        onOpenSecurityLog = { navController.navigate(SECURITY_LOG_ROUTE) },
                    )
                }

                composable(SECURITY_LOG_ROUTE) {
                    SecurityLogScreen(onBack = { navController.popBackStack() })
                }

                composable(OPEN_REQUEST_ROUTE) {
                    OpenRequestPrompt(onDone = { navController.popBackStack() }, viewModel = openRequestViewModel)
                }

                composable(ADMIN_ROUTE) {
                    AdminUsersScreen(
                        onBack = { navController.popBackStack() },
                        onOpenUser = { id -> navController.navigate("admin/user/$id") },
                    )
                }

                composable("admin/user/{userId}") {
                    AdminUserScreen(
                        onBack = { navController.popBackStack() },
                        onSetUpPhone = { id -> navController.navigate("admin/user/$id/setup") },
                    )
                }

                composable("admin/user/{userId}/setup") {
                    AdminSetupPhoneScreen(onDone = { navController.popBackStack() })
                }

                composable(ADD_PHONE_ROUTE) {
                    AddPhoneScreen(mode = IssuerMode.Own, onDone = { navController.popBackStack() })
                }

                composable(SCAN_SETUP_ROUTE) {
                    SetupScanScreen(
                        onSignedIn = { navController.navigate("home") { popUpTo(0) { inclusive = true } } },
                        onBack = { navController.popBackStack() },
                    )
                }

                composable(SETUP_FLOW_ROUTE) {
                    // Read during composition, cleared only once composed (a discarded composition
                    // never loses the memory-only code); a newer link opened here replaces it.
                    // Memory only (never saved state): the setup code is a secret.
                    var entry by remember { mutableStateOf<SetupEntry?>(null) }
                    val shown = pendingSetup ?: entry
                    LaunchedEffect(pendingSetup) {
                        pendingSetup?.let {
                            entry = it
                            PendingSetupLinks.consume(it)
                        }
                    }
                    SetupFlowScreen(
                        entry = shown,
                        onSignedIn = { navController.navigate("home") { popUpTo(0) { inclusive = true } } },
                        onBack = { if (!navController.popBackStack()) navController.navigate("login") },
                        // Restored without its (memory-only) setup code: scan it again.
                        onRescan = {
                            navController.navigate(SCAN_SETUP_ROUTE) { popUpTo(SETUP_FLOW_ROUTE) { inclusive = true } }
                        },
                    )
                }

                composable(SETUP_PENDING_ROUTE) {
                    SetupPendingScreen(
                        onSignedIn = { navController.navigate("home") { popUpTo(0) { inclusive = true } } },
                        onEnded = { navController.navigate("login") { popUpTo(0) { inclusive = true } } },
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

/** Setting up this phone from a setup code or link (L2-A1). */
const val SETUP_FLOW_ROUTE = "setup_flow"

/** A setup that waits for its ready time (L2-A13; E1's start destination for `Pending`). */
const val SETUP_PENDING_ROUTE = "setup_pending"

/** "Add a phone" on a key phone (F5a issuer). */
const val ADD_PHONE_ROUTE = "add_phone"

/** Security settings on the phone (Task B11). */
const val SECURITY_SETTINGS_ROUTE = "settings/security"

/** The read-only security log (a security notification opens it). */
const val SECURITY_LOG_ROUTE = "security_log"

/** The full-screen open-request prompt (L2-A10). */
const val OPEN_REQUEST_ROUTE = "open_request"

/** Running ceremonies the open-request prompt never interrupts (it follows once they end). */
val CEREMONY_ROUTES = setOf(
    SCAN_ROUTE, APPROVE_LOGIN_ROUTE, ADD_PHONE_ROUTE, CONFIRM_RENEW_ROUTE, "admin/user/{userId}/setup",
)

/** The small admin area on the admin's own key phone (F5c, coordinator call 1). */
const val ADMIN_ROUTE = "admin"
