package io.wolftown.kaiku.data.api

import io.ktor.client.*
import io.ktor.client.call.*
import io.ktor.client.request.*
import io.ktor.http.*
import io.wolftown.kaiku.domain.model.Category
import io.wolftown.kaiku.domain.model.Guild
import javax.inject.Inject

interface GuildApi {
    suspend fun getGuilds(): List<Guild>
    suspend fun getGuild(guildId: String): Guild
    suspend fun getCategories(guildId: String): List<Category>
}

class GuildApiImpl @Inject constructor(
    private val httpClient: HttpClient
) : GuildApi {

    override suspend fun getGuilds(): List<Guild> {
        val response = httpClient.get("/api/guilds")

        if (!response.status.isSuccess()) {
            val errorBody = runCatching { response.body<ApiErrorResponse>() }.getOrNull()
            throw ApiException(response.status, errorBody?.message ?: "Failed to load guilds")
        }

        return response.body()
    }

    override suspend fun getGuild(guildId: String): Guild {
        val response = httpClient.get("/api/guilds/$guildId")

        if (!response.status.isSuccess()) {
            val errorBody = runCatching { response.body<ApiErrorResponse>() }.getOrNull()
            throw ApiException(response.status, errorBody?.message ?: "Failed to load guild")
        }

        return response.body()
    }

    override suspend fun getCategories(guildId: String): List<Category> {
        val response = httpClient.get("/api/guilds/$guildId/categories")

        if (!response.status.isSuccess()) {
            val errorBody = runCatching { response.body<ApiErrorResponse>() }.getOrNull()
            throw ApiException(response.status, errorBody?.message ?: "Failed to load categories")
        }

        return response.body()
    }
}
