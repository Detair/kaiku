package io.wolftown.kaiku.di

import dagger.Binds
import dagger.Module
import dagger.hilt.InstallIn
import dagger.hilt.components.SingletonComponent
import io.wolftown.kaiku.data.api.ApprovalApi
import io.wolftown.kaiku.data.api.ApprovalApiImpl
import io.wolftown.kaiku.data.api.EnrollmentApi
import io.wolftown.kaiku.data.api.EnrollmentApiImpl
import io.wolftown.kaiku.data.api.LoginAttemptApi
import io.wolftown.kaiku.data.api.LoginAttemptApiImpl
import io.wolftown.kaiku.data.api.SecurityApi
import io.wolftown.kaiku.data.api.SecurityApiImpl
import javax.inject.Singleton

/** C26–C28 API clients (the v1.0 login). */
@Module
@InstallIn(SingletonComponent::class)
abstract class LoginApiModule {
    @Binds @Singleton abstract fun bindApprovalApi(impl: ApprovalApiImpl): ApprovalApi
    @Binds @Singleton abstract fun bindEnrollmentApi(impl: EnrollmentApiImpl): EnrollmentApi
    @Binds @Singleton abstract fun bindLoginAttemptApi(impl: LoginAttemptApiImpl): LoginAttemptApi
    @Binds @Singleton abstract fun bindSecurityApi(impl: SecurityApiImpl): SecurityApi
}
