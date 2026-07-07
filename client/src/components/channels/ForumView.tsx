/**
 * ForumView — the content view for a forum channel: a list of post cards and a
 * "New Post" composer. Replies reuse the existing thread system (a post's root
 * message opens as a thread); this view covers browsing + creating posts.
 */

import { Component, For, Show, createResource, createSignal } from "solid-js";
import { MessageSquare, Plus, Tag as TagIcon, Pin, Lock } from "lucide-solid";
import {
  listForumPosts,
  createForumPost,
  listForumTags,
  type ForumPost,
} from "@/lib/tauri";
import { formatRelativeTimeShort } from "@/lib/utils";

const ForumView: Component<{ channelId: string }> = (props) => {
  const [activeTag, setActiveTag] = createSignal<string | undefined>(undefined);
  const [composing, setComposing] = createSignal(false);
  const [title, setTitle] = createSignal("");
  const [content, setContent] = createSignal("");
  const [submitting, setSubmitting] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const [posts, { refetch }] = createResource(
    () => ({ ch: props.channelId, tag: activeTag() }),
    (k) => listForumPosts(k.ch, k.tag),
  );
  const [tags] = createResource(() => props.channelId, listForumTags);

  const submit = async (e: Event) => {
    e.preventDefault();
    if (!title().trim() || !content().trim() || submitting()) return;
    setSubmitting(true);
    setError(null);
    try {
      await createForumPost(props.channelId, {
        title: title().trim(),
        content: content().trim(),
      });
      setTitle("");
      setContent("");
      setComposing(false);
      await refetch();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create post");
    } finally {
      setSubmitting(false);
    }
  };

  const tagName = (id: string) =>
    tags()?.find((t) => t.id === id)?.name ?? "tag";

  return (
    <div class="flex-1 overflow-y-auto p-4">
      {/* Header actions */}
      <div class="flex items-center justify-between mb-4">
        <div class="flex flex-wrap gap-1.5">
          <button
            class="px-2.5 py-1 rounded-full text-xs font-medium transition-colors"
            classList={{
              "bg-accent-primary/20 text-text-primary": activeTag() === undefined,
              "bg-[var(--color-surface-layer2)] text-text-secondary hover:text-text-primary":
                activeTag() !== undefined,
            }}
            onClick={() => setActiveTag(undefined)}
          >
            All
          </button>
          <For each={tags()}>
            {(t) => (
              <button
                class="px-2.5 py-1 rounded-full text-xs font-medium flex items-center gap-1 transition-colors"
                classList={{
                  "bg-accent-primary/20 text-text-primary": activeTag() === t.id,
                  "bg-[var(--color-surface-layer2)] text-text-secondary hover:text-text-primary":
                    activeTag() !== t.id,
                }}
                onClick={() => setActiveTag(t.id)}
              >
                <TagIcon class="w-3 h-3" />
                {t.name}
              </button>
            )}
          </For>
        </div>
        <button
          class="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-accent-primary text-on-accent text-sm font-medium hover:bg-accent-primary/90 transition-colors"
          onClick={() => setComposing((c) => !c)}
        >
          <Plus class="w-4 h-4" />
          New Post
        </button>
      </div>

      {/* Composer */}
      <Show when={composing()}>
        <form
          onSubmit={submit}
          class="mb-4 p-4 rounded-lg border border-white/10 space-y-3"
          style={{ "background-color": "var(--color-surface-layer1)" }}
        >
          <input
            class="w-full px-3 py-2 rounded-lg text-sm text-text-primary bg-[var(--color-surface-layer2)] border border-white/10 outline-none focus:border-accent-primary"
            type="text"
            placeholder="Post title"
            maxlength={128}
            value={title()}
            onInput={(e) => setTitle(e.currentTarget.value)}
          />
          <textarea
            class="w-full px-3 py-2 rounded-lg text-sm text-text-primary bg-[var(--color-surface-layer2)] border border-white/10 outline-none focus:border-accent-primary min-h-[100px]"
            placeholder="Write your post…"
            value={content()}
            onInput={(e) => setContent(e.currentTarget.value)}
          />
          <Show when={error()}>
            <div class="text-xs text-accent-danger">{error()}</div>
          </Show>
          <div class="flex justify-end gap-2">
            <button
              type="button"
              class="px-3 py-1.5 rounded-lg text-sm text-text-secondary hover:text-text-primary"
              onClick={() => setComposing(false)}
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={!title().trim() || !content().trim() || submitting()}
              class="px-3 py-1.5 rounded-lg bg-accent-primary text-on-accent text-sm font-medium hover:bg-accent-primary/90 disabled:opacity-50 disabled:cursor-not-allowed"
            >
              Post
            </button>
          </div>
        </form>
      </Show>

      {/* Post cards */}
      <Show
        when={!posts.loading}
        fallback={<div class="text-center py-8 text-text-secondary">Loading…</div>}
      >
        <Show
          when={(posts()?.length ?? 0) > 0}
          fallback={
            <div class="text-center py-12 text-text-secondary text-sm">
              No posts yet. Start the first one!
            </div>
          }
        >
          <div class="space-y-2">
            <For each={posts()}>
              {(p: ForumPost) => (
                <div
                  class="p-3 rounded-lg border border-white/10 hover:bg-white/5 transition-colors"
                  style={{ "background-color": "var(--color-surface-layer1)" }}
                >
                  <div class="flex items-center gap-2">
                    <Show when={p.pinned}>
                      <Pin class="w-3.5 h-3.5 text-accent-primary" />
                    </Show>
                    <Show when={p.locked}>
                      <Lock class="w-3.5 h-3.5 text-text-muted" />
                    </Show>
                    <span class="font-medium text-text-primary truncate">
                      {p.title}
                    </span>
                  </div>
                  <div class="flex items-center gap-3 mt-1 text-xs text-text-secondary">
                    <span class="flex items-center gap-1">
                      <MessageSquare class="w-3 h-3" />
                      {p.reply_count}
                    </span>
                    <span>{formatRelativeTimeShort(p.last_activity_at)}</span>
                    <For each={p.tag_ids}>
                      {(id) => (
                        <span class="px-1.5 py-0.5 rounded bg-accent-primary/20 text-text-primary">
                          {tagName(id)}
                        </span>
                      )}
                    </For>
                  </div>
                </div>
              )}
            </For>
          </div>
        </Show>
      </Show>
    </div>
  );
};

export default ForumView;
