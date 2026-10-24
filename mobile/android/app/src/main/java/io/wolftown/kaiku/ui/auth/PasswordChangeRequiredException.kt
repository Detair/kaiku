package io.wolftown.kaiku.ui.auth

/**
 * Thrown by [io.wolftown.kaiku.data.repository.AuthRepository.login] when the
 * server reports `password_change_required` (C4): the account still uses the
 * temporary password an admin set, and the user must choose their own before
 * the session can be used. Nothing has been persisted at this point.
 */
class PasswordChangeRequiredException : Exception("Password change required")
