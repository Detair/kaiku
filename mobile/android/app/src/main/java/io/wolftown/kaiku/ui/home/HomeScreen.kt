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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import io.wolftown.kaiku.R
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
    val sections by viewModel.sections.collectAsState()
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
                        Text(selectedGuild?.name ?: stringResource(R.string.app_name))
                    },
                    navigationIcon = {
                        IconButton(onClick = { scope.launch { drawerState.open() } }) {
                            Icon(Icons.Default.Menu, contentDescription = stringResource(R.string.home_open_servers))
                        }
                    },
                    actions = {
                        IconButton(onClick = onNavigateToDms) {
                            Icon(Icons.Default.Email, contentDescription = stringResource(R.string.home_direct_messages))
                        }
                        IconButton(onClick = { viewModel.refresh() }) {
                            Icon(Icons.Default.Refresh, contentDescription = stringResource(R.string.home_refresh))
                        }
                        IconButton(onClick = onNavigateToSettings) {
                            Icon(Icons.Default.Settings, contentDescription = stringResource(R.string.home_settings))
                        }
                    }
                )
            }
        ) { paddingValues ->
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(paddingValues)
            ) {
            // A failed load is shown inline while there is still something to
            // display; the full-screen error below is only for the empty case.
            val errorRes = error
            if (errorRes != null && guilds.isNotEmpty()) {
                ErrorRow(
                    message = stringResource(errorRes),
                    onRetry = { viewModel.retry() }
                )
            }
            Box(
                modifier = Modifier
                    .weight(1f)
                    .fillMaxWidth()
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
                                text = stringResource(error ?: R.string.home_servers_load_failed),
                                color = MaterialTheme.colorScheme.error,
                                style = MaterialTheme.typography.bodyLarge
                            )
                            Spacer(modifier = Modifier.height(16.dp))
                            Button(onClick = { viewModel.retry() }) {
                                Text(stringResource(R.string.common_retry))
                            }
                        }
                    }

                    selectedGuild == null && guilds.isNotEmpty() -> {
                        Column(
                            modifier = Modifier.align(Alignment.Center),
                            horizontalAlignment = Alignment.CenterHorizontally
                        ) {
                            Text(
                                text = stringResource(R.string.home_select_server),
                                style = MaterialTheme.typography.bodyLarge,
                                color = MaterialTheme.colorScheme.onSurfaceVariant
                            )
                            Spacer(modifier = Modifier.height(8.dp))
                            TextButton(onClick = { scope.launch { drawerState.open() } }) {
                                Text(stringResource(R.string.home_open_server_list))
                            }
                        }
                    }

                    selectedGuild == null && guilds.isEmpty() && !isLoading -> {
                        Text(
                            text = stringResource(R.string.home_no_servers),
                            modifier = Modifier.align(Alignment.Center),
                            style = MaterialTheme.typography.bodyLarge,
                            color = MaterialTheme.colorScheme.onSurfaceVariant
                        )
                    }

                    else -> {
                        ChannelList(
                            sections = sections,
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
}

@Composable
private fun ErrorRow(message: String, onRetry: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Text(
            text = message,
            modifier = Modifier.weight(1f),
            color = MaterialTheme.colorScheme.error,
            style = MaterialTheme.typography.bodyMedium
        )
        TextButton(onClick = onRetry) {
            Text(stringResource(R.string.common_retry))
        }
    }
}
