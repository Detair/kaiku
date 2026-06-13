package io.wolftown.kaiku.ui

import android.net.Uri
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.ui.auth.LoginScreen
import io.wolftown.kaiku.ui.auth.QrRedeemScreen
import io.wolftown.kaiku.ui.auth.QrScannerScreen
import io.wolftown.kaiku.ui.auth.RegisterScreen
import io.wolftown.kaiku.ui.auth.ServerUrlScreen
import io.wolftown.kaiku.ui.channel.TextChannelScreen
import io.wolftown.kaiku.ui.dm.DmListScreen
import io.wolftown.kaiku.ui.home.HomeScreen
import io.wolftown.kaiku.ui.settings.SettingsScreen
import io.wolftown.kaiku.ui.voice.VoiceChannelScreen
import io.wolftown.kaiku.ui.voice.VoiceOverlay
import io.wolftown.kaiku.ui.voice.VoiceOverlayViewModel

@Composable
fun KaikuNavGraph(
    navController: NavHostController,
    startDestination: String,
    authState: AuthState,
    appVersion: String = ""
) {
    val currentUserId by authState.currentUserId.collectAsState()
    val isLoggedIn by authState.isLoggedIn.collectAsState()
    val navBackStackEntry by navController.currentBackStackEntryAsState()
    val currentRoute = navBackStackEntry?.destination?.route

    // Auth guard: redirect to login when logged out from an authenticated screen
    val authRoutes = setOf("server_url", "login", "register", "qr_scanner", "qr_redeem/{serverUrl}/{token}")
    LaunchedEffect(isLoggedIn) {
        if (!isLoggedIn && currentRoute != null && currentRoute !in authRoutes) {
            navController.navigate("login") {
                popUpTo(0) { inclusive = true }
            }
        }
    }

    // Voice overlay state — shows on non-voice screens when connected
    val voiceOverlayViewModel: VoiceOverlayViewModel = hiltViewModel()
    val voiceChannelId by voiceOverlayViewModel.currentChannelId.collectAsState()
    val voiceIsMuted by voiceOverlayViewModel.isMuted.collectAsState()
    val showOverlay = voiceChannelId != null && currentRoute != "voice/{channelId}"

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
                            navController.navigate("qr_scanner")
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
                        }
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
                        }
                    )
                }

                composable("dms") {
                    DmListScreen(
                        onOpenDm = { channelId ->
                            navController.navigate("channel/$channelId")
                        },
                        onNavigateBack = { navController.popBackStack() }
                    )
                }

                composable("channel/{channelId}") {
                    TextChannelScreen(
                        currentUserId = currentUserId ?: "",
                        onNavigateBack = { navController.popBackStack() }
                    )
                }

                composable("voice/{channelId}") {
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
                        },
                        onScanQrCode = {
                            navController.navigate("qr_scanner")
                        }
                    )
                }

                composable("qr_scanner") {
                    QrScannerScreen(
                        onQrScanned = { serverUrl, token ->
                            navController.navigate(
                                "qr_redeem/${Uri.encode(serverUrl)}/${Uri.encode(token)}"
                            )
                        },
                        onNavigateBack = { navController.popBackStack() }
                    )
                }

                composable("qr_redeem/{serverUrl}/{token}") { backStackEntry ->
                    val serverUrl = Uri.decode(
                        backStackEntry.arguments?.getString("serverUrl") ?: ""
                    )
                    val token = Uri.decode(
                        backStackEntry.arguments?.getString("token") ?: ""
                    )
                    QrRedeemScreen(
                        serverUrl = serverUrl,
                        token = token,
                        onSuccess = {
                            navController.navigate("home") {
                                popUpTo(0) { inclusive = true }
                            }
                        },
                        onError = { navController.popBackStack() }
                    )
                }
            }
        }

        // Voice overlay bar — visible on non-voice screens when in a voice channel
        if (showOverlay) {
            VoiceOverlay(
                channelName = voiceChannelId ?: "",
                isMuted = voiceIsMuted,
                onMuteToggle = { voiceOverlayViewModel.onToggleMute() },
                onTap = {
                    voiceChannelId?.let { channelId ->
                        navController.navigate("voice/$channelId")
                    }
                },
                onDisconnect = { voiceOverlayViewModel.onDisconnect() }
            )
        }
    }
}
