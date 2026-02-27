/**
 * CategoryManager — Modal component for category CRUD operations.
 *
 * Provides inline rename, creation, and deletion with confirmation.
 */

import { type Component, createSignal, For, Show } from "solid-js";
import { Check, Pencil, Plus, Trash2, X } from "lucide-solid";
import type { PageCategory } from "@/lib/types";
import { MAX_CATEGORY_NAME_LENGTH } from "@/lib/pageConstants";

interface CategoryManagerProps {
  guildId: string;
  categories: PageCategory[];
  onClose: () => void;
  onCreateCategory: (name: string) => Promise<void>;
  onUpdateCategory: (categoryId: string, name: string) => Promise<void>;
  onDeleteCategory: (categoryId: string) => Promise<void>;
}

const CategoryManager: Component<CategoryManagerProps> = (props) => {
  const [editingId, setEditingId] = createSignal<string | null>(null);
  const [editingName, setEditingName] = createSignal("");
  const [newCategoryName, setNewCategoryName] = createSignal("");
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const clearError = () => setError(null);

  const validateName = (name: string): string | null => {
    const trimmed = name.trim();
    if (trimmed.length === 0) {
      return "Category name cannot be empty.";
    }
    if (trimmed.length > MAX_CATEGORY_NAME_LENGTH) {
      return `Category name must be at most ${MAX_CATEGORY_NAME_LENGTH} characters.`;
    }
    return null;
  };

  const handleCreate = async () => {
    const trimmed = newCategoryName().trim();
    const validationError = validateName(trimmed);
    if (validationError) {
      setError(validationError);
      return;
    }

    clearError();
    setLoading(true);
    try {
      await props.onCreateCategory(trimmed);
      setNewCategoryName("");
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Failed to create category.",
      );
    } finally {
      setLoading(false);
    }
  };

  const startEditing = (category: PageCategory) => {
    setEditingId(category.id);
    setEditingName(category.name);
    clearError();
  };

  const cancelEditing = () => {
    setEditingId(null);
    setEditingName("");
  };

  const handleUpdate = async () => {
    const id = editingId();
    if (!id) return;

    const trimmed = editingName().trim();
    const validationError = validateName(trimmed);
    if (validationError) {
      setError(validationError);
      return;
    }

    clearError();
    setLoading(true);
    try {
      await props.onUpdateCategory(id, trimmed);
      setEditingId(null);
      setEditingName("");
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Failed to update category.",
      );
    } finally {
      setLoading(false);
    }
  };

  const handleDelete = async (categoryId: string, categoryName: string) => {
    const confirmed = confirm(
      `Delete category "${categoryName}"? Pages in this category will become uncategorized.`,
    );
    if (!confirmed) return;

    clearError();
    setLoading(true);
    try {
      await props.onDeleteCategory(categoryId);
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Failed to delete category.",
      );
    } finally {
      setLoading(false);
    }
  };

  const handleBackdropClick = (e: MouseEvent) => {
    if (e.target === e.currentTarget) {
      props.onClose();
    }
  };

  const handleEditKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter") {
      e.preventDefault();
      handleUpdate();
    } else if (e.key === "Escape") {
      cancelEditing();
    }
  };

  const handleCreateKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter") {
      e.preventDefault();
      handleCreate();
    }
  };

  return (
    <div
      class="fixed inset-0 bg-black/50 flex items-center justify-center z-50"
      onClick={handleBackdropClick}
    >
      <div class="bg-zinc-800 rounded-lg shadow-xl w-full max-w-md mx-4">
        {/* Header */}
        <div class="flex items-center justify-between px-6 py-4 border-b border-zinc-700">
          <h2 class="text-lg font-semibold text-white">Manage Categories</h2>
          <button
            onClick={() => props.onClose()}
            class="text-zinc-400 hover:text-white rounded-md p-1 transition-colors"
            aria-label="Close"
          >
            <X size={20} />
          </button>
        </div>

        {/* Body */}
        <div class="px-6 py-4 max-h-96 overflow-y-auto">
          {/* Error display */}
          <Show when={error()}>
            <div class="mb-3 px-3 py-2 bg-red-900/30 border border-red-700 rounded-md text-red-300 text-sm">
              {error()}
            </div>
          </Show>

          {/* Category list */}
          <div class="space-y-1">
            <For each={props.categories}>
              {(category) => (
                <div class="flex items-center gap-3 px-3 py-2 rounded-md hover:bg-zinc-700/50">
                  <Show
                    when={editingId() === category.id}
                    fallback={
                      <>
                        {/* Display mode */}
                        <span class="flex-1 text-white truncate">
                          {category.name}
                        </span>
                        <button
                          onClick={() => startEditing(category)}
                          class="text-zinc-400 hover:text-white p-1 rounded-md transition-colors flex-shrink-0"
                          disabled={loading()}
                          aria-label={`Rename ${category.name}`}
                        >
                          <Pencil size={14} />
                        </button>
                        <button
                          onClick={() =>
                            handleDelete(category.id, category.name)
                          }
                          class="text-red-400 hover:text-red-300 hover:bg-red-900/30 p-1 rounded-md transition-colors flex-shrink-0"
                          disabled={loading()}
                          aria-label={`Delete ${category.name}`}
                        >
                          <Trash2 size={14} />
                        </button>
                      </>
                    }
                  >
                    {/* Edit mode */}
                    <input
                      type="text"
                      value={editingName()}
                      onInput={(e) => setEditingName(e.currentTarget.value)}
                      onKeyDown={handleEditKeyDown}
                      maxLength={MAX_CATEGORY_NAME_LENGTH}
                      class="flex-1 px-2 py-1 bg-zinc-900 border border-zinc-600 rounded-md text-white text-sm focus:outline-none focus:ring-2 focus:ring-emerald-500 focus:border-transparent"
                      disabled={loading()}
                      autofocus
                    />
                    <button
                      onClick={() => handleUpdate()}
                      class="bg-emerald-600 hover:bg-emerald-500 text-white p-1 rounded-md transition-colors flex-shrink-0"
                      disabled={loading()}
                      aria-label="Save"
                    >
                      <Check size={14} />
                    </button>
                    <button
                      onClick={cancelEditing}
                      class="text-zinc-400 hover:text-white p-1 rounded-md transition-colors flex-shrink-0"
                      disabled={loading()}
                      aria-label="Cancel"
                    >
                      <X size={14} />
                    </button>
                  </Show>
                </div>
              )}
            </For>
          </div>

          {/* Empty state */}
          <Show when={props.categories.length === 0}>
            <p class="text-zinc-500 text-sm text-center py-4">
              No categories yet. Create one below.
            </p>
          </Show>
        </div>

        {/* Footer — Add category */}
        <div class="px-6 py-4 border-t border-zinc-700">
          <div class="flex items-center gap-2">
            <input
              type="text"
              value={newCategoryName()}
              onInput={(e) => setNewCategoryName(e.currentTarget.value)}
              onKeyDown={handleCreateKeyDown}
              maxLength={MAX_CATEGORY_NAME_LENGTH}
              placeholder="New category name..."
              class="w-full px-3 py-2 bg-zinc-900 border border-zinc-600 rounded-md text-white placeholder-zinc-500 focus:outline-none focus:ring-2 focus:ring-emerald-500 focus:border-transparent"
              disabled={loading()}
            />
            <button
              onClick={() => handleCreate()}
              class="bg-emerald-600 hover:bg-emerald-500 text-white p-2 rounded-md transition-colors flex-shrink-0"
              disabled={loading() || newCategoryName().trim().length === 0}
              aria-label="Add category"
            >
              <Plus size={18} />
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};

export default CategoryManager;
