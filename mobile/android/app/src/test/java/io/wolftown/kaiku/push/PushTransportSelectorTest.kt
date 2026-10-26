package io.wolftown.kaiku.push

import io.wolftown.kaiku.data.api.FcmPublicConfig
import io.wolftown.kaiku.data.api.PushConfig
import org.junit.Assert.assertEquals
import org.junit.Test

class PushTransportSelectorTest {

    private val bothProviders = PushConfig(
        providers = listOf("fcm", "unifiedpush"),
        fcm = FcmPublicConfig(projectId = PROJECT),
    )

    private fun env(
        gms: Boolean = true,
        project: String? = PROJECT,
        server: PushConfig = bothProviders,
        distributors: List<String> = emptyList(),
        preferUp: Boolean = false,
        saved: String? = null,
    ) = PushEnvironment(
        gmsAvailable = gms,
        firebaseProjectId = project,
        server = server,
        distributors = distributors,
        preferUnifiedPush = preferUp,
        savedDistributor = saved,
    )

    @Test
    fun `gms phone with matching project uses fcm`() {
        assertEquals(TransportChoice.Fcm, PushTransportSelector.select(env()))
    }

    @Test
    fun `project mismatch falls back to unified push`() {
        val choice = PushTransportSelector.select(
            env(project = "someone-else", distributors = listOf(NTFY))
        )
        assertEquals(TransportChoice.UnifiedPush(NTFY), choice)
    }

    @Test
    fun `apk without firebase config falls back to unified push`() {
        val choice = PushTransportSelector.select(env(project = null, distributors = listOf(NTFY)))
        assertEquals(TransportChoice.UnifiedPush(NTFY), choice)
    }

    @Test
    fun `apk without firebase config and no distributor is off`() {
        assertEquals(
            TransportChoice.Off(PushOffReason.NO_TRANSPORT),
            PushTransportSelector.select(env(project = null)),
        )
    }

    @Test
    fun `server without fcm uses unified push`() {
        val server = PushConfig(providers = listOf("unifiedpush"), fcm = null)
        val choice = PushTransportSelector.select(env(server = server, distributors = listOf(NTFY)))
        assertEquals(TransportChoice.UnifiedPush(NTFY), choice)
    }

    @Test
    fun `server listing fcm without a project does not use fcm`() {
        val server = PushConfig(providers = listOf("fcm"), fcm = null)
        assertEquals(
            TransportChoice.Off(PushOffReason.NO_TRANSPORT),
            PushTransportSelector.select(env(server = server)),
        )
    }

    @Test
    fun `no gms and no distributor is off with no transport`() {
        assertEquals(
            TransportChoice.Off(PushOffReason.NO_TRANSPORT),
            PushTransportSelector.select(env(gms = false)),
        )
    }

    @Test
    fun `distributor without server support is off`() {
        val server = PushConfig(providers = listOf("fcm"), fcm = FcmPublicConfig(PROJECT))
        assertEquals(
            TransportChoice.Off(PushOffReason.NO_TRANSPORT),
            PushTransportSelector.select(env(gms = false, server = server, distributors = listOf(NTFY))),
        )
    }

    @Test
    fun `prefer unified push honored only when distributor installed`() {
        assertEquals(
            TransportChoice.UnifiedPush(NTFY),
            PushTransportSelector.select(env(distributors = listOf(NTFY), preferUp = true)),
        )
        assertEquals(TransportChoice.Fcm, PushTransportSelector.select(env(preferUp = true)))
    }

    @Test
    fun `saved distributor wins while installed`() {
        val installed = listOf(NTFY, OTHER)
        assertEquals(
            TransportChoice.UnifiedPush(OTHER),
            PushTransportSelector.select(env(gms = false, distributors = installed, saved = OTHER)),
        )
        assertEquals(
            TransportChoice.UnifiedPush(NTFY),
            PushTransportSelector.select(env(gms = false, distributors = installed, saved = "gone.app")),
        )
    }

    private companion object {
        const val PROJECT = "kaiku-prod"
        const val NTFY = "io.heckel.ntfy"
        const val OTHER = "org.unifiedpush.distributor.other"
    }
}
