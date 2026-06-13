package io.wolftown.kaiku.ui.home

import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Email
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import io.wolftown.kaiku.domain.model.ChannelType
import kotlinx.coroutines.launch

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HomeScreen(
    onNavigateToTextChannel: (channelId: String) -> Unit,
    onNavigateToVoiceChannel: (channelId: String) -> Unit,
    onNavigateToSettings: () -> Unit = {},
    onNavigateToDms: () -> Unit = {},
    viewModel: HomeViewModel = hiltViewModel()
) {
    val guilds by viewModel.guilds.collectAsState()
    val selectedGuild by viewModel.selectedGuild.collectAsState()
    val channels by viewModel.channels.collectAsState()
    val isLoading by viewModel.isLoading.collectAsState()
    val error by viewModel.error.collectAsState()

    val drawerState = rememberDrawerState(initialValue = DrawerValue.Closed)
    val scope = rememberCoroutineScope()

    // Handle channel navigation events
    LaunchedEffect(Unit) {
        viewModel.navigateToChannel.collect { event ->
            when (event.channelType) {
                ChannelType.VOICE -> onNavigateToVoiceChannel(event.channelId)
                else -> onNavigateToTextChannel(event.channelId)
            }
        }
    }

    ModalNavigationDrawer(
        drawerState = drawerState,
        drawerContent = {
            ModalDrawerSheet(
                modifier = Modifier.width(80.dp)
            ) {
                GuildSidebar(
                    guilds = guilds,
                    selectedGuildId = selectedGuild?.id,
                    onGuildSelected = { guildId ->
                        viewModel.onGuildSelected(guildId)
                        scope.launch { drawerState.close() }
                    }
                )
            }
        }
    ) {
        Scaffold(
            topBar = {
                TopAppBar(
                    title = {
                        Text(selectedGuild?.name ?: "Kaiku")
                    },
                    navigationIcon = {
                        IconButton(onClick = { scope.launch { drawerState.open() } }) {
                            Icon(Icons.Default.Menu, contentDescription = "Open guilds")
                        }
                    },
                    actions = {
                        IconButton(onClick = onNavigateToDms) {
                            Icon(Icons.Default.Email, contentDescription = "Direct messages")
                        }
                        IconButton(onClick = { viewModel.refresh() }) {
                            Icon(Icons.Default.Refresh, contentDescription = "Refresh")
                        }
                        IconButton(onClick = onNavigateToSettings) {
                            Icon(Icons.Default.Settings, contentDescription = "Settings")
                        }
                    }
                )
            }
        ) { paddingValues ->
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(paddingValues)
            ) {
                when {
                    isLoading && guilds.isEmpty() -> {
                        CircularProgressIndicator(
                            modifier = Modifier.align(Alignment.Center)
                        )
                    }

                    error != null && guilds.isEmpty() -> {
                        Column(
                            modifier = Modifier.align(Alignment.Center),
                            horizontalAlignment = Alignment.CenterHorizontally
                        ) {
                            Text(
                                text = error ?: "An error occurred",
                                color = MaterialTheme.colorScheme.error,
                                style = MaterialTheme.typography.bodyLarge
                            )
                            Spacer(modifier = Modifier.height(16.dp))
                            Button(onClick = { viewModel.refresh() }) {
                                Text("Retry")
                            }
                        }
                    }

                    selectedGuild == null && guilds.isNotEmpty() -> {
                        Column(
                            modifier = Modifier.align(Alignment.Center),
                            horizontalAlignment = Alignment.CenterHorizontally
                        ) {
                            Text(
                                text = "Select a guild to get started",
                                style = MaterialTheme.typography.bodyLarge,
                                color = MaterialTheme.colorScheme.onSurfaceVariant
                            )
                            Spacer(modifier = Modifier.height(8.dp))
                            TextButton(onClick = { scope.launch { drawerState.open() } }) {
                                Text("Open guild list")
                            }
                        }
                    }

                    selectedGuild == null && guilds.isEmpty() && !isLoading -> {
                        Text(
                            text = "No guilds available",
                            modifier = Modifier.align(Alignment.Center),
                            style = MaterialTheme.typography.bodyLarge,
                            color = MaterialTheme.colorScheme.onSurfaceVariant
                        )
                    }

                    else -> {
                        ChannelList(
                            channels = channels,
                            onChannelSelected = { channelId, channelType ->
                                viewModel.onChannelSelected(channelId, channelType)
                            }
                        )
                    }
                }
            }
        }
    }
}
