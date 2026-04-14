package io.wolftown.kaiku.ui.voice

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Call
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import io.wolftown.kaiku.data.voice.AudioRoute
import io.wolftown.kaiku.data.ws.ScreenShareInfo
import io.wolftown.kaiku.data.ws.VoiceParticipant
import org.webrtc.VideoTrack

/**
 * Full-screen voice channel view.
 *
 * Layout:
 * - Top bar with channel name and back button (leaves voice on back)
 * - Screen share area (placeholder, shown when active)
 * - Participant grid: 2-column LazyVerticalGrid
 * - Bottom bar: mute toggle, audio route picker, disconnect button
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun VoiceChannelScreen(
    channelName: String,
    onNavigateBack: () -> Unit,
    viewModel: VoiceViewModel = hiltViewModel()
) {
    val participants by viewModel.participants.collectAsState()
    val isMuted by viewModel.isMuted.collectAsState()
    val isConnected by viewModel.isConnected.collectAsState()
    val screenShares by viewModel.screenShares.collectAsState()
    val remoteVideoTracks by viewModel.remoteVideoTracks.collectAsState()
    val layerPreferences by viewModel.layerPreferences.collectAsState()
    val currentRoute by viewModel.currentRoute.collectAsState()
    val availableRoutes by viewModel.availableRoutes.collectAsState()

    var fullscreenStreamId by remember { mutableStateOf<String?>(null) }

    // Fullscreen mode: show only the screen share
    if (fullscreenStreamId != null) {
        val share = screenShares.find { it.streamId == fullscreenStreamId }
        if (share != null) {
            val videoTrack = findVideoTrackForStream(remoteVideoTracks, share.streamId)
            val currentLayer = layerPreferences[share.streamId] ?: "auto"
            ScreenShareView(
                videoTrack = videoTrack,
                screenShareInfo = share,
                eglContext = viewModel.eglContext,
                currentLayer = currentLayer,
                onLayerChange = { viewModel.onSetLayerPreference(share.streamId, it) },
                onToggleFullscreen = { fullscreenStreamId = null },
                modifier = Modifier.fillMaxSize()
            )
            return
        } else {
            // Screen share ended while in fullscreen — exit fullscreen
            fullscreenStreamId = null
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("#$channelName") },
                navigationIcon = {
                    IconButton(onClick = {
                        viewModel.onLeave()
                        onNavigateBack()
                    }) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = "Leave and go back"
                        )
                    }
                }
            )
        },
        bottomBar = {
            VoiceBottomBar(
                isMuted = isMuted,
                currentRoute = currentRoute,
                availableRoutes = availableRoutes,
                onToggleMute = viewModel::onToggleMute,
                onSwitchRoute = viewModel::onSwitchAudioRoute,
                onDisconnect = {
                    viewModel.onLeave()
                    onNavigateBack()
                }
            )
        }
    ) { paddingValues ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(paddingValues)
        ) {
            // Screen share area
            if (screenShares.isNotEmpty()) {
                ScreenShareArea(
                    screenShares = screenShares,
                    remoteVideoTracks = remoteVideoTracks,
                    layerPreferences = layerPreferences,
                    eglContext = viewModel.eglContext,
                    onLayerChange = viewModel::onSetLayerPreference,
                    onToggleFullscreen = { streamId -> fullscreenStreamId = streamId }
                )
            }

            // Connection status
            if (!isConnected) {
                LinearProgressIndicator(
                    modifier = Modifier.fillMaxWidth()
                )
            }

            // Participant grid
            if (participants.isEmpty()) {
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .weight(1f),
                    contentAlignment = Alignment.Center
                ) {
                    Text(
                        text = "No one else is here yet",
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                }
            } else {
                LazyVerticalGrid(
                    columns = GridCells.Fixed(2),
                    modifier = Modifier
                        .fillMaxSize()
                        .weight(1f),
                    contentPadding = PaddingValues(16.dp),
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp)
                ) {
                    items(participants, key = { it.userId }) { participant ->
                        ParticipantCard(participant = participant)
                    }
                }
            }
        }
    }
}

@Composable
private fun ParticipantCard(participant: VoiceParticipant) {
    val isSpeaking = !participant.muted
    val borderColor = if (isSpeaking) {
        MaterialTheme.colorScheme.primary
    } else {
        Color.Transparent
    }

    Card(
        modifier = Modifier
            .fillMaxWidth()
            .aspectRatio(1f),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceVariant
        )
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(12.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center
        ) {
            // Avatar circle
            Box(
                modifier = Modifier
                    .size(64.dp)
                    .clip(CircleShape)
                    .background(MaterialTheme.colorScheme.primary.copy(alpha = 0.2f))
                    .border(
                        width = if (isSpeaking) 3.dp else 0.dp,
                        color = borderColor,
                        shape = CircleShape
                    ),
                contentAlignment = Alignment.Center
            ) {
                Text(
                    text = (participant.displayName ?: participant.username ?: "?")
                        .take(1)
                        .uppercase(),
                    style = MaterialTheme.typography.headlineMedium,
                    color = MaterialTheme.colorScheme.primary
                )
            }

            Spacer(modifier = Modifier.height(8.dp))

            // Display name
            Text(
                text = participant.displayName ?: participant.username ?: "Unknown",
                style = MaterialTheme.typography.bodyMedium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                textAlign = TextAlign.Center
            )

            // Mute indicator
            if (participant.muted) {
                Spacer(modifier = Modifier.height(4.dp))
                Text(
                    text = "Muted",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
            }
        }
    }
}

/**
 * Screen share area with support for multiple concurrent screen shares.
 *
 * Single share: renders directly. Multiple shares: uses [HorizontalPager]
 * with page indicators. Takes ~60% of screen height when active.
 */
@Composable
private fun ScreenShareArea(
    screenShares: List<ScreenShareInfo>,
    remoteVideoTracks: Map<String, VideoTrack>,
    layerPreferences: Map<String, String>,
    eglContext: org.webrtc.EglBase.Context,
    onLayerChange: (String, String) -> Unit,
    onToggleFullscreen: (String) -> Unit
) {
    if (screenShares.size == 1) {
        // Single screen share — render directly
        val share = screenShares.first()
        val videoTrack = findVideoTrackForStream(remoteVideoTracks, share.streamId)
        val currentLayer = layerPreferences[share.streamId] ?: "auto"

        ScreenShareView(
            videoTrack = videoTrack,
            screenShareInfo = share,
            eglContext = eglContext,
            currentLayer = currentLayer,
            onLayerChange = { layer -> onLayerChange(share.streamId, layer) },
            onToggleFullscreen = { onToggleFullscreen(share.streamId) },
            modifier = Modifier
                .fillMaxWidth()
                .fillMaxHeight(0.6f)
                .padding(horizontal = 8.dp, vertical = 4.dp)
        )
    } else {
        // Multiple screen shares — horizontal pager
        val pagerState = rememberPagerState(pageCount = { screenShares.size })

        Column(
            modifier = Modifier
                .fillMaxWidth()
                .fillMaxHeight(0.6f)
        ) {
            HorizontalPager(
                state = pagerState,
                modifier = Modifier
                    .fillMaxWidth()
                    .weight(1f)
                    .padding(horizontal = 8.dp, vertical = 4.dp)
            ) { page ->
                val share = screenShares[page]
                val videoTrack = findVideoTrackForStream(remoteVideoTracks, share.streamId)
                val currentLayer = layerPreferences[share.streamId] ?: "auto"

                ScreenShareView(
                    videoTrack = videoTrack,
                    screenShareInfo = share,
                    eglContext = eglContext,
                    currentLayer = currentLayer,
                    onLayerChange = { layer -> onLayerChange(share.streamId, layer) },
                    onToggleFullscreen = { onToggleFullscreen(share.streamId) },
                    modifier = Modifier.fillMaxSize()
                )
            }

            // Page indicators
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(vertical = 4.dp),
                horizontalArrangement = Arrangement.Center
            ) {
                repeat(screenShares.size) { index ->
                    val color = if (pagerState.currentPage == index) {
                        MaterialTheme.colorScheme.primary
                    } else {
                        MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.3f)
                    }
                    Box(
                        modifier = Modifier
                            .padding(horizontal = 3.dp)
                            .size(6.dp)
                            .clip(CircleShape)
                            .background(color)
                    )
                }
            }
        }
    }
}

/**
 * Finds the remote video track for a given screen share stream ID.
 *
 * The server labels video tracks with the stream ID in the track ID or mid.
 * Returns null if no matching track is found — no fallback to avoid
 * mismatching tracks when multiple screen shares are active.
 */
private fun findVideoTrackForStream(
    remoteVideoTracks: Map<String, VideoTrack>,
    streamId: String
): VideoTrack? {
    // Direct match by track ID containing the stream ID
    return remoteVideoTracks.entries.find { (trackId, _) ->
        trackId.contains(streamId)
    }?.value
}

@Composable
private fun VoiceBottomBar(
    isMuted: Boolean,
    currentRoute: AudioRoute,
    availableRoutes: Set<AudioRoute>,
    onToggleMute: () -> Unit,
    onSwitchRoute: (AudioRoute) -> Unit,
    onDisconnect: () -> Unit
) {
    var showRouteMenu by remember { mutableStateOf(false) }

    Surface(
        tonalElevation = 3.dp,
        modifier = Modifier.fillMaxWidth()
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 24.dp, vertical = 12.dp),
            horizontalArrangement = Arrangement.SpaceEvenly,
            verticalAlignment = Alignment.CenterVertically
        ) {
            // Mute/unmute toggle
            FilledIconToggleButton(
                checked = isMuted,
                onCheckedChange = { onToggleMute() }
            ) {
                if (isMuted) {
                    Text("🔇", style = MaterialTheme.typography.titleLarge)
                } else {
                    Text("🎤", style = MaterialTheme.typography.titleLarge)
                }
            }

            // Audio route picker
            Box {
                IconButton(onClick = { showRouteMenu = true }) {
                    Text(
                        text = when (currentRoute) {
                            AudioRoute.Speaker -> "🔊"
                            AudioRoute.Earpiece -> "📱"
                            AudioRoute.Bluetooth -> "🎧"
                            AudioRoute.WiredHeadset -> "🎧"
                        },
                        style = MaterialTheme.typography.titleLarge
                    )
                }

                DropdownMenu(
                    expanded = showRouteMenu,
                    onDismissRequest = { showRouteMenu = false }
                ) {
                    availableRoutes.forEach { route ->
                        DropdownMenuItem(
                            text = {
                                Row(verticalAlignment = Alignment.CenterVertically) {
                                    Text(
                                        text = when (route) {
                                            AudioRoute.Speaker -> "🔊 Speaker"
                                            AudioRoute.Earpiece -> "📱 Earpiece"
                                            AudioRoute.Bluetooth -> "🎧 Bluetooth"
                                            AudioRoute.WiredHeadset -> "🎧 Wired Headset"
                                        }
                                    )
                                    if (route == currentRoute) {
                                        Spacer(modifier = Modifier.width(8.dp))
                                        Text(
                                            text = "✓",
                                            color = MaterialTheme.colorScheme.primary
                                        )
                                    }
                                }
                            },
                            onClick = {
                                onSwitchRoute(route)
                                showRouteMenu = false
                            }
                        )
                    }
                }
            }

            // Disconnect button
            FilledIconButton(
                onClick = onDisconnect,
                colors = IconButtonDefaults.filledIconButtonColors(
                    containerColor = MaterialTheme.colorScheme.error,
                    contentColor = MaterialTheme.colorScheme.onError
                )
            ) {
                Icon(
                    imageVector = Icons.Default.Call,
                    contentDescription = "Disconnect"
                )
            }
        }
    }
}
