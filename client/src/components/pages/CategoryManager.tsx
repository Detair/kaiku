/**
 * CategoryManager — Modal component for category CRUD operations.
 *
 * Provides inline rename, creation, and deletion with confirmation.
 */

import { type Component, createSignal, For, Show } from "solid-js";
import { Check, Pencil, Plus, Trash2, X } from "lucide-solid";
import type { PageCategory } from "@/lib/types";
import { MAX_CATEGORY_NAME_LENGTH } from "@/lib/pageConstants";
import { createConfirm } from "@/components/admin/confirmHost";
import { actionErrorText } from "@/components/admin/actionErrors";
import { t } from "@/i18n";

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
        actionErrorText(err),
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
        actionErrorText(err),
      );
    } finally {
      setLoading(false);
    }
  };

  const confirmDialog = createConfirm();
  const handleDelete = (categoryId: string, categoryName: string) =>
    confirmDialog.ask({
      title: t("guildSettings.pages.deleteCategoryTitle", { name: categoryName }),
      message: t("guildSettings.pages.deleteCategoryBody", { name: categoryName }),
      confirmLabel: t("guildSettings.categories.delete"),
      danger: true,
      run: () => deleteNow(categoryId),
    });

  const deleteNow = async (categoryId: string) => {
    clearError();
    setLoading(true);
    try {
      await props.onDeleteCategory(categoryId);
    } catch (err) {
      setError(
        actionErrorText(err),
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
      <div class="bg-surface-layer1 rounded-lg shadow-xl w-full max-w-md mx-4">
        {/* Header */}
        <div class="flex items-center justify-between px-6 py-4 border-b border-white/10">
          <h2 class="text-lg font-semibold text-text-primary">Manage Categories</h2>
          <button
            onClick={() => props.onClose()}
            class="text-text-secondary hover:text-text-primary rounded-md p-1 transition-colors"
            aria-label="Close"
          >
            <X size={20} />
          </button>
        </div>

        {/* Body */}
        <div class="px-6 py-4 max-h-96 overflow-y-auto">
          {/* Error display */}
          <Show when={error()}>
            <div class="mb-3 px-3 py-2 bg-status-error/15 border border-status-error/50 rounded-md text-text-primary text-sm">
              {error()}
            </div>
          </Show>

          {/* Category list */}
          <div class="space-y-1">
            <For each={props.categories}>
              {(category) => (
                <div class="flex items-center gap-3 px-3 py-2 rounded-md hover:bg-white/10">
                  <Show
                    when={editingId() === category.id}
                    fallback={
                      <>
                        {/* Display mode */}
                        <span class="flex-1 text-text-primary truncate">
                          {category.name}
                        </span>
                        <button
                          onClick={() => startEditing(category)}
                          class="text-text-secondary hover:text-text-primary p-1 rounded-md transition-colors flex-shrink-0 state-disabled-quiet"
                          disabled={loading()}
                          aria-label={`Rename ${category.name}`}
                        >
                          <Pencil size={14} />
                        </button>
                        <button
                          onClick={() =>
                            handleDelete(category.id, category.name)
                          }
                          class="text-text-primary hover:bg-status-error/15 p-1 rounded-md transition-colors flex-shrink-0 state-disabled-quiet"
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
                      class="flex-1 px-2 py-1 bg-surface-base border border-white/15 rounded-md text-text-primary text-sm focus:outline-none focus:ring-2 focus:ring-accent-primary focus:border-transparent"
                      disabled={loading()}
                      autofocus
                    />
                    <button
                      onClick={() => handleUpdate()}
                      class="bg-accent-primary hover:bg-accent-primary/90 text-on-accent p-1 rounded-md transition-colors flex-shrink-0 state-disabled"
                      disabled={loading()}
                      aria-label="Save"
                    >
                      <Check size={14} />
                    </button>
                    <button
                      onClick={cancelEditing}
                      class="text-text-secondary hover:text-text-primary p-1 rounded-md transition-colors flex-shrink-0 state-disabled-quiet"
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
            <p class="text-text-secondary text-sm text-center py-4">
              No categories yet. Create one below.
            </p>
          </Show>
        </div>

        {/* Footer — Add category */}
        <div class="px-6 py-4 border-t border-white/10">
          <div class="flex items-center gap-2">
            <input
              type="text"
              value={newCategoryName()}
              onInput={(e) => setNewCategoryName(e.currentTarget.value)}
              onKeyDown={handleCreateKeyDown}
              maxLength={MAX_CATEGORY_NAME_LENGTH}
              placeholder="New category name..."
              class="w-full px-3 py-2 bg-surface-base border border-white/15 rounded-md text-text-primary placeholder-text-placeholder focus:outline-none focus:ring-2 focus:ring-accent-primary focus:border-transparent"
              disabled={loading()}
            />
            <button
              onClick={() => handleCreate()}
              class="bg-accent-primary hover:bg-accent-primary/90 text-on-accent p-2 rounded-md transition-colors flex-shrink-0 state-disabled"
              disabled={loading() || newCategoryName().trim().length === 0}
              aria-label="Add category"
            >
              <Plus size={18} />
            </button>
          </div>
        </div>
      </div>
      <confirmDialog.Host />
    </div>
  );
};

export default CategoryManager;
