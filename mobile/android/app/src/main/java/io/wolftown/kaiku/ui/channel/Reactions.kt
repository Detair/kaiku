package io.wolftown.kaiku.ui.channel

/**
 * Curated set of quick-reaction emoji shown in the [EmojiPicker]. Kept small
 * and ordered by expected frequency so the most common reactions are reachable
 * without scrolling. Pure data (no Compose dependency) so it can be unit tested.
 */
val COMMON_REACTIONS: List<String> = listOf(
    "👍", // thumbs up
    "👎", // thumbs down
    "❤️", // red heart
    "😂", // joy
    "😮", // open mouth
    "😢", // crying
    "😡", // angry
    "🎉", // party popper
    "🔥", // fire
    "👏", // clapping
    "🙏", // folded hands
    "💯", // hundred points
    "🚀", // rocket
    "👀", // eyes
    "✅", // check mark
    "❓", // question mark
)
