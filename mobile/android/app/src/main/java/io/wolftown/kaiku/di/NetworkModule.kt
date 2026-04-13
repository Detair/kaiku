package io.wolftown.kaiku.di

import dagger.Module
import dagger.Provides
import dagger.hilt.InstallIn
import dagger.hilt.components.SingletonComponent
import io.ktor.client.*
import io.wolftown.kaiku.data.KaikuJson
import io.wolftown.kaiku.data.api.KaikuHttpClient
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import kotlinx.serialization.json.Json
import okhttp3.ConnectionSpec
import okhttp3.TlsVersion
import javax.inject.Singleton

@Module
@InstallIn(SingletonComponent::class)
object NetworkModule {

    @Provides
    @Singleton
    fun provideTls13Spec(): ConnectionSpec {
        return ConnectionSpec.Builder(ConnectionSpec.MODERN_TLS)
            .tlsVersions(TlsVersion.TLS_1_3)
            .build()
    }

    @Provides
    @Singleton
    fun provideKaikuHttpClient(
        tokenStorage: TokenStorage,
        authState: AuthState,
        tls13Spec: ConnectionSpec
    ): KaikuHttpClient {
        return KaikuHttpClient(tokenStorage, authState, tls13Spec)
    }

    @Provides
    @Singleton
    fun provideHttpClient(kaikuHttpClient: KaikuHttpClient): HttpClient {
        return kaikuHttpClient.httpClient
    }

    @Provides
    @Singleton
    fun provideJson(): Json = KaikuJson
}
