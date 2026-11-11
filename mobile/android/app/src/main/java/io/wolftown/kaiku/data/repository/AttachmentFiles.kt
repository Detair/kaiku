package io.wolftown.kaiku.data.repository

import android.content.ContentValues
import android.content.Context
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import androidx.annotation.RequiresApi
import androidx.core.content.FileProvider
import dagger.hilt.android.qualifiers.ApplicationContext
import io.wolftown.kaiku.data.api.AuthHeaderInterceptor
import io.wolftown.kaiku.data.api.ServerUrls
import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.di.ChatCoroutineScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.launch
import io.wolftown.kaiku.domain.model.Attachment
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.isActive
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.File
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.util.concurrent.TimeUnit
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Files of received attachments (AND-B-4): downloads them into the app's cache
 * (with the same bearer handling as images), hands them to other apps through
 * the [FileProvider], and saves them where the user finds them, all without a
 * storage permission: MediaStore on Android 10+, the system "save as" dialog
 * before that (the screen asks for the target, [copyTo] writes it).
 */
@Singleton
class AttachmentFiles internal constructor(
    private val context: Context,
    okHttpClient: OkHttpClient,
    private val tokenStorage: TokenStorage,
    private val authState: AuthState,
    /** Background deletion of cleared folders; the session-end observer runs here too. */
    private val scope: CoroutineScope,
    /** Tests: answers requests after the auth interceptor, instead of the network. */
    transport: okhttp3.Interceptor?,
) {

    @Inject
    constructor(
        @ApplicationContext context: Context,
        okHttpClient: OkHttpClient,
        tokenStorage: TokenStorage,
        authState: AuthState,
        @ChatCoroutineScope scope: CoroutineScope,
    ) : this(context, okHttpClient, tokenStorage, authState, scope, null)

    /**
     * A download's claim: the session it started in and the cache generation it
     * writes into. It stays valid only while that session is current, has not
     * ended, and the cache was not cleared since ([isValid]).
     */
    private class Ticket(val sessionId: Long, val generation: Long)

    /** Guards [generation], [endedSessions], [calls] and every folder create/rename. */
    private val lock = Any()
    private var generation = 0L
    private val endedSessions = HashSet<Long>()
    private val calls = HashMap<okhttp3.Call, Ticket>()

    /** Refuses a request whose session is no longer valid, before and after the token is added. */
    private val sessionGuard = okhttp3.Interceptor { chain ->
        val ticket = chain.request().tag(Ticket::class.java)
        if (ticket == null || !isValid(ticket)) throw IOException("session ended; download not sent")
        chain.proceed(chain.request())
    }

    private val client: OkHttpClient = okHttpClient.newBuilder()
        // The shared client has no read timeout (WebSocket); downloads must not hang forever.
        .readTimeout(DOWNLOAD_READ_TIMEOUT_SECONDS, TimeUnit.SECONDS)
        .addInterceptor(sessionGuard)
        .addInterceptor(AuthHeaderInterceptor(tokenStorage))
        // Checked again after the token was read, like KaikuHttpClient's RequiredSessionId.
        .addInterceptor(sessionGuard)
        .apply { if (transport != null) addInterceptor(transport) }
        .build()

    init {
        // Every ended session (never conflated): nothing of it may be downloaded or kept.
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            authState.endedSessions.collect { ended -> endSession(ended.sessionId) }
        }
        // Folders a previous process cleared but could not finish deleting.
        scope.launch { deleteTrash() }
    }

    private fun currentSessionId(): Long? = (authState.session.value as? AuthSession.LoggedIn)?.sessionId

    private fun isValid(ticket: Ticket): Boolean = synchronized(lock) {
        ticket.generation == generation &&
            ticket.sessionId !in endedSessions &&
            ticket.sessionId == currentSessionId()
    }

    private fun requireValid(ticket: Ticket) {
        if (!isValid(ticket)) throw CancellationException("session ended; download dropped")
    }

    /** Session [sessionId] ended: its downloads stop and its files go. */
    fun endSession(sessionId: Long) {
        synchronized(lock) { endedSessions += sessionId }
        clearCache()
    }

    /**
     * Deletes every downloaded attachment (session teardown, via
     * [ChatRepository.clearSession], and every ended session). Running downloads
     * are cancelled and can no longer create, write or finish a file: the cache
     * generation changes under the same lock that guards folder creation and the
     * final rename. The folder is moved aside at once (nothing reachable for the
     * next session, nothing shared by the FileProvider) and deleted in the
     * background, so teardown never waits for large files.
     */
    fun clearCache() {
        val running = synchronized(lock) {
            generation++
            val dir = baseDir
            if (dir.exists()) {
                val trash = File(context.cacheDir, "$TRASH_PREFIX${System.nanoTime()}")
                if (!dir.renameTo(trash)) dir.deleteRecursively()
            }
            calls.keys.toList().also { calls.clear() }
        }
        running.forEach { it.cancel() }
        scope.launch { deleteTrash() }
    }

    private fun deleteTrash() {
        context.cacheDir.listFiles { f -> f.name.startsWith(TRASH_PREFIX) }?.forEach { it.deleteRecursively() }
    }

    private val baseDir: File get() = File(context.cacheDir, CACHE_DIR)

    /**
     * The cached copy of [attachment], downloading it first when needed. Bound
     * to the session it starts in; a [CancellationException] when that session
     * ends or the cache is cleared meanwhile.
     */
    suspend fun download(attachment: Attachment): File = withContext(Dispatchers.IO) {
        val ticket = synchronized(lock) {
            val session = currentSessionId() ?: throw IOException("not signed in")
            Ticket(session, generation)
        }
        requireValid(ticket)
        val target = AttachmentPaths.cacheFile(baseDir, attachment.id, attachment.filename)
        if (target.isFile && (attachment.size <= 0 || target.length() == attachment.size)) return@withContext target
        val url = ServerUrls.resolve(tokenStorage.getServerUrl(), attachment.url)
            ?: throw IOException("attachment has no usable url")
        // Created only while the claim holds: a cleared cache is never brought back.
        synchronized(lock) {
            requireValid(ticket)
            target.parentFile?.mkdirs()
        }
        val partial = File(target.parentFile, target.name + ".part")
        var promoted = false
        val call = client.newCall(Request.Builder().url(url).tag(Ticket::class.java, ticket).build())
        synchronized(lock) {
            requireValid(ticket)
            calls[call] = ticket
        }
        try {
            call.execute().use { response ->
                if (!response.isSuccessful) throw IOException("download failed: ${response.code}")
                val body = response.body ?: throw IOException("empty download")
                val limit = AttachmentPaths.downloadLimit(attachment.size)
                requireValid(ticket)
                body.byteStream().use { input ->
                    partial.outputStream().use { output ->
                        copyLimited(input, output, limit) { !isActive || !isValid(ticket) }
                    }
                }
            }
            // The finished file appears only if the claim still holds at this moment.
            synchronized(lock) {
                requireValid(ticket)
                if (!partial.renameTo(target)) throw IOException("cannot store download")
            }
            promoted = true
        } finally {
            synchronized(lock) { calls.remove(call) }
            // Failure, size limit, cancellation or a cleared cache: no half file stays behind.
            if (!promoted) partial.delete()
        }
        target
    }

    /** A `content://` address other apps may read once granted (open, share). */
    fun shareableUri(file: File): Uri =
        FileProvider.getUriForFile(context, "${context.packageName}$AUTHORITY_SUFFIX", file)

    /**
     * Saves [file] to the shared Pictures, Movies or Download folder (subfolder
     * "Kaiku") on Android 10+. Returns false below Android 10: the caller asks
     * for a target with the system dialog and uses [copyTo].
     */
    suspend fun saveToDevice(file: File, attachment: Attachment): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return false
        withContext(Dispatchers.IO) { insertIntoMediaStore(file, attachment) }
        return true
    }

    /** Writes [file] to [target] (from the system "save as" dialog). */
    suspend fun copyTo(file: File, target: Uri) = withContext(Dispatchers.IO) {
        val out = context.contentResolver.openOutputStream(target) ?: throw IOException("cannot open target")
        out.use { output -> file.inputStream().use { it.copyTo(output) } }
    }

    @RequiresApi(Build.VERSION_CODES.Q)
    private fun insertIntoMediaStore(file: File, attachment: Attachment) {
        val mime = attachment.mimeType.lowercase()
        val (collection, folder) = when {
            mime.startsWith("image/") ->
                MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY) to Environment.DIRECTORY_PICTURES
            mime.startsWith("video/") ->
                MediaStore.Video.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY) to Environment.DIRECTORY_MOVIES
            else ->
                MediaStore.Downloads.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY) to Environment.DIRECTORY_DOWNLOADS
        }
        val values = ContentValues().apply {
            put(MediaStore.MediaColumns.DISPLAY_NAME, file.name)
            put(MediaStore.MediaColumns.MIME_TYPE, attachment.mimeType)
            put(MediaStore.MediaColumns.RELATIVE_PATH, "$folder/Kaiku")
            put(MediaStore.MediaColumns.IS_PENDING, 1)
        }
        val resolver = context.contentResolver
        val item = resolver.insert(collection, values) ?: throw IOException("cannot create media entry")
        try {
            val out = resolver.openOutputStream(item) ?: throw IOException("cannot open media entry")
            out.use { output -> file.inputStream().use { it.copyTo(output) } }
            resolver.update(item, ContentValues().apply { put(MediaStore.MediaColumns.IS_PENDING, 0) }, null, null)
        } catch (e: Exception) {
            resolver.delete(item, null, null)
            throw e
        }
    }

    private fun copyLimited(input: InputStream, output: OutputStream, limit: Long, cancelled: () -> Boolean) {
        val buffer = ByteArray(BUFFER_SIZE)
        var total = 0L
        while (true) {
            // Leaving the screen stops the download between chunks.
            if (cancelled()) throw kotlinx.coroutines.CancellationException("download cancelled")
            val n = input.read(buffer)
            if (n < 0) break
            total += n
            if (total > limit) throw IOException("download larger than announced")
            output.write(buffer, 0, n)
        }
    }

    private companion object {
        const val CACHE_DIR = "attachments"
        const val TRASH_PREFIX = "attachments-cleared-"
        const val AUTHORITY_SUFFIX = ".files"
        const val DOWNLOAD_READ_TIMEOUT_SECONDS = 60L
        const val BUFFER_SIZE = 64 * 1024
    }
}

/** Where downloads go; pure so the path rules can be unit tested. */
object AttachmentPaths {
    private const val MAX_NAME = 100

    /** Without a known size a download may not exceed this. */
    const val UNKNOWN_SIZE_LIMIT = 512L * 1024 * 1024

    /** Allowed bytes for a download announced with [declaredSize] (some slack for a wrong size). */
    fun downloadLimit(declaredSize: Long): Long =
        if (declaredSize <= 0) UNKNOWN_SIZE_LIMIT else declaredSize + 1024 * 1024

    /**
     * A file name that cannot leave its folder: no path parts, no leading dots,
     * only letters, digits and `._- ` kept, at most [MAX_NAME] characters.
     */
    fun safeName(name: String, fallback: String = "file"): String {
        val last = name.substringAfterLast('/').substringAfterLast('\\')
        val cleaned = last.map { c -> if (c.isLetterOrDigit() || c in "._- ") c else '_' }
            .joinToString("")
            .trim()
            .trimStart('.')
        val extension = cleaned.substringAfterLast('.', "").take(10)
        val shortened = if (cleaned.length <= MAX_NAME) cleaned
        else cleaned.take(MAX_NAME - extension.length - 1) + "." + extension
        return shortened.ifBlank { fallback }
    }

    /** `<base>/<attachment id>/<file name>`, always inside [base]. */
    fun cacheFile(base: File, attachmentId: String, fileName: String): File {
        val dir = File(base, safeName(attachmentId, fallback = "attachment"))
        val file = File(dir, safeName(fileName))
        val canonicalBase = base.canonicalFile
        require(file.canonicalFile.toPath().startsWith(canonicalBase.toPath())) { "attachment path escapes the cache" }
        return file
    }
}
