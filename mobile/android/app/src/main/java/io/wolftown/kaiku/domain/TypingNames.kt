package io.wolftown.kaiku.domain

/**
 * Display names for the user [ids] that are typing. Ids never appear in the
 * result: an id missing from [members] (user id to display name) shows as
 * [someone]. Order follows [ids].
 */
fun typingNames(ids: Set<String>, members: Map<String, String>, someone: String): List<String> =
    ids.map { id -> members[id]?.takeIf { it.isNotBlank() } ?: someone }
