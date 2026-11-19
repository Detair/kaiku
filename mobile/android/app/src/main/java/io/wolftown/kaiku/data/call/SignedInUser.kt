package io.wolftown.kaiku.data.call

import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.AuthState

/**
 * The signed-in user right now, read from the session itself: unlike
 * [AuthState.currentUserId], which follows a moment later on its scope, it is
 * already set when a screen opened from a notification has just restored
 * the session.
 */
fun AuthState.signedInUserId(): String? = (session.value as? AuthSession.LoggedIn)?.userId
