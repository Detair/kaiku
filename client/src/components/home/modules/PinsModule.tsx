/**
 * PinsModule Component
 *
 * Shows user's pinned notes, links, and messages.
 */

import { Component, Show, For, createSignal, onMount } from "solid-js";
import {
  Pin,
  FileText,
  Link,
  MessageSquare,
  Plus,
  Trash2,
  ExternalLink,
} from "lucide-solid";
import { pins, loadPins, createPin, deletePin, updatePin } from "@/stores/pins";
import type { Pin as PinItem, PinType } from "@/lib/types";
import CollapsibleModule from "./CollapsibleModule";

const PinsModule: Component = () => {
  const [isAdding, setIsAdding] = createSignal(false);
  const [addType, setAddType] = createSignal<PinType>("note");
  const [newContent, setNewContent] = createSignal("");
  const [newTitle, setNewTitle] = createSignal("");
  const [editingId, setEditingId] = createSignal<string | null>(null);
  const [editContent, setEditContent] = createSignal("");

  // Load pins on mount
  onMount(() => {
    loadPins();
  });

  const handleCreate = async () => {
    if (!newContent().trim()) return;

    await createPin({
      pin_type: addType(),
      content: newContent(),
      title: newTitle() || undefined,
    });

    setNewContent("");
    setNewTitle("");
    setIsAdding(false);
  };

  const handleDelete = async (pinId: string) => {
    await deletePin(pinId);
  };

  const startEdit = (pin: PinItem) => {
    setEditingId(pin.id);
    setEditContent(pin.content);
  };

  const saveEdit = async (pinId: string) => {
    await updatePin(pinId, { content: editContent() });
    setEditingId(null);
  };

  const cancelEdit = () => {
    setEditingId(null);
    setEditContent("");
  };

  const getPinIcon = (type: PinType) => {
    switch (type) {
      case "note":
        return <FileText class="w-4 h-4" />;
      case "link":
        return <Link class="w-4 h-4" />;
      case "message":
        return <MessageSquare class="w-4 h-4" />;
    }
  };

  return (
    <CollapsibleModule id="pins" title="Pins" badge={pins().length}>
      <div class="space-y-2">
        {/* Add button */}
        <Show when={!isAdding()}>
          <button
            onClick={() => setIsAdding(true)}
            class="w-full flex items-center justify-center gap-2 py-2 px-3 rounded-lg border border-dashed border-white/20 text-text-secondary hover:border-white/40 hover:text-text-primary transition-colors"
          >
            <Plus class="w-4 h-4" />
            <span class="text-sm">Add Pin</span>
          </button>
        </Show>

        {/* Add form */}
        <Show when={isAdding()}>
          <div class="p-3 rounded-lg bg-white/5 space-y-2">
            <div class="flex gap-2">
              <button
                onClick={() => setAddType("note")}
                class={`flex-1 py-1.5 px-2 rounded text-xs font-medium transition-colors ${
                  addType() === "note"
                    ? "bg-accent-primary text-white"
                    : "bg-white/10 text-text-secondary hover:bg-white/20"
                }`}
              >
                Note
              </button>
              <button
                onClick={() => setAddType("link")}
                class={`flex-1 py-1.5 px-2 rounded text-xs font-medium transition-colors ${
                  addType() === "link"
                    ? "bg-accent-primary text-white"
                    : "bg-white/10 text-text-secondary hover:bg-white/20"
                }`}
              >
                Link
              </button>
            </div>
            <Show when={addType() === "link"}>
              <input
                type="text"
                placeholder="Title (optional)"
                value={newTitle()}
                onInput={(e) => setNewTitle(e.currentTarget.value)}
                class="w-full px-3 py-1.5 rounded bg-surface-base border border-white/10 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:border-accent-primary"
              />
            </Show>
            <textarea
              placeholder={addType() === "link" ? "URL" : "Note content..."}
              value={newContent()}
              onInput={(e) => setNewContent(e.currentTarget.value)}
              rows={addType() === "note" ? 3 : 1}
              class="w-full px-3 py-1.5 rounded bg-surface-base border border-white/10 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:border-accent-primary resize-none"
            />
            <div class="flex justify-end gap-2">
              <button
                onClick={() => setIsAdding(false)}
                class="px-3 py-1.5 rounded text-sm text-text-secondary hover:bg-white/10"
              >
                Cancel
              </button>
              <button
                onClick={handleCreate}
                disabled={!newContent().trim()}
                class="px-3 py-1.5 rounded text-sm bg-accent-primary text-white hover:bg-accent-primary/80 disabled:opacity-50 disabled:cursor-not-allowed"
              >
                Save
              </button>
            </div>
          </div>
        </Show>

        {/* Pins list */}
        <For each={pins()}>
          {(pin) => (
            <div class="group relative py-2 px-2 rounded-lg hover:bg-white/5">
              <Show
                when={editingId() === pin.id}
                fallback={
                  <div class="flex items-start gap-2">
                    <span class="text-text-secondary mt-0.5">
                      {getPinIcon(pin.pin_type)}
                    </span>
                    <div class="flex-1 min-w-0">
                      <Show when={pin.title}>
                        <div class="text-sm font-medium text-text-primary truncate">
                          {pin.title}
                        </div>
                      </Show>
                      <Show
                        when={pin.pin_type === "link"}
                        fallback={
                          <p
                            class="text-sm text-text-secondary line-clamp-2 cursor-pointer"
                            onClick={() => startEdit(pin)}
                          >
                            {pin.content}
                          </p>
                        }
                      >
                        <a
                          href={pin.content}
                          target="_blank"
                          rel="noopener noreferrer"
                          class="text-sm text-accent-primary hover:underline flex items-center gap-1"
                        >
                          <span class="truncate">{pin.content}</span>
                          <ExternalLink class="w-3 h-3 flex-shrink-0" />
                        </a>
                      </Show>
                    </div>
                    <button
                      onClick={() => handleDelete(pin.id)}
                      class="opacity-0 group-hover:opacity-100 p-1 rounded text-text-muted hover:text-status-error hover:bg-status-error/10 transition-all"
                      title="Delete"
                    >
                      <Trash2 class="w-4 h-4" />
                    </button>
                  </div>
                }
              >
                {/* Edit mode */}
                <div class="space-y-2">
                  <textarea
                    value={editContent()}
                    onInput={(e) => setEditContent(e.currentTarget.value)}
                    rows={3}
                    class="w-full px-3 py-1.5 rounded bg-surface-base border border-white/10 text-sm text-text-primary focus:outline-none focus:border-accent-primary resize-none"
                  />
                  <div class="flex justify-end gap-2">
                    <button
                      onClick={cancelEdit}
                      class="px-2 py-1 rounded text-xs text-text-secondary hover:bg-white/10"
                    >
                      Cancel
                    </button>
                    <button
                      onClick={() => saveEdit(pin.id)}
                      class="px-2 py-1 rounded text-xs bg-accent-primary text-white hover:bg-accent-primary/80"
                    >
                      Save
                    </button>
                  </div>
                </div>
              </Show>
            </div>
          )}
        </For>

        {/* Empty state */}
        <Show when={pins().length === 0 && !isAdding()}>
          <div class="text-center py-4">
            <Pin class="w-8 h-8 text-text-secondary mx-auto mb-2 opacity-50" />
            <p class="text-sm text-text-secondary">No pins yet</p>
            <p class="text-xs text-text-muted mt-1">
              Save notes and links for quick access
            </p>
          </div>
        </Show>
      </div>
    </CollapsibleModule>
  );
};

export default PinsModule;
