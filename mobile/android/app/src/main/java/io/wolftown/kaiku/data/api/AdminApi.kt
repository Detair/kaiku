package io.wolftown.kaiku.data.api

import io.ktor.client.HttpClient
import io.ktor.client.request.post
import javax.inject.Inject

/** Admin routes of the v1.0 login used on the phone (C26, C27). */
interface AdminApi {
    /** "Remove device now" (C26, L1b): another account's key device or passkey; needs `admin.authenticator.remove`. */
    suspend fun removeAuthenticator(userId: String, authenticatorId: String)
}

class AdminApiImpl @Inject constructor(private val httpClient: HttpClient) : AdminApi {
    override suspend fun removeAuthenticator(userId: String, authenticatorId: String) {
        httpClient.post("/api/admin/users/$userId/authenticators/$authenticatorId/remove").ensureSuccess("remove failed")
    }
}
