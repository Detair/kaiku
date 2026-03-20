/**
 * CategoriesTab - Category management in guild settings
 *
 * Provides CRUD for channel categories:
 * - Create categories and subcategories
 * - Inline rename (click pencil, Enter to save, Escape to cancel)
 * - Delete with confirmation
 */

import { Component, createSignal, For, Show, onMount } from "solid-js";
import { Pencil, Trash2, Plus, Check, X, ChevronRight, Hash, Volume2, Layers } from "lucide-solid";
import {
  categoriesState,
  loadGuildCategories,
  getTopLevelCategories,
  getSubcategories,
  createCategory,
  updateCategory,
  deleteCategory,
} from "@/stores/categories";

interface CategoriesTabProps {
  guildId: string;
}

const CategoriesTab: Component<CategoriesTabProps> = (props) => {
  const [newCategoryName, setNewCategoryName] = createSignal("");
  const [editingId, setEditingId] = createSignal<string | null>(null);
  const [editingName, setEditingName] = createSignal("");
  const [deletingId, setDeletingId] = createSignal<string | null>(null);
  const [addingSubcategoryFor, setAddingSubcategoryFor] = createSignal<string | null>(null);
  const [subCategoryName, setSubCategoryName] = createSignal("");
  const [isCreating, setIsCreating] = createSignal(false);
  const [newCategoryType, setNewCategoryType] = createSignal<"mixed" | "text" | "voice">("mixed");

  onMount(() => {
    loadGuildCategories(props.guildId);
  });

  const topLevel = () => getTopLevelCategories(props.guildId);

  const typeIcon = (type: string) => {
    switch (type) {
      case "text": return <Hash class="w-3.5 h-3.5" />;
      case "voice": return <Volume2 class="w-3.5 h-3.5" />;
      default: return <Layers class="w-3.5 h-3.5" />;
    }
  };

  const typeLabel = (type: string) => {
    switch (type) {
      case "text": return "Text only";
      case "voice": return "Voice only";
      default: return "Mixed";
    }
  };

  const handleCreate = async () => {
    const name = newCategoryName().trim();
    if (!name) return;
    setIsCreating(true);
    await createCategory(props.guildId, name, undefined, newCategoryType());
    setNewCategoryName("");
    setNewCategoryType("mixed");
    setIsCreating(false);
  };

  const handleCreateSubcategory = async (parentId: string) => {
    const name = subCategoryName().trim();
    if (!name) return;
    setIsCreating(true);
    await createCategory(props.guildId, name, parentId);
    setSubCategoryName("");
    setAddingSubcategoryFor(null);
    setIsCreating(false);
  };

  const startEditing = (id: string, currentName: string) => {
    setEditingId(id);
    setEditingName(currentName);
  };

  const handleRename = async () => {
    const id = editingId();
    const name = editingName().trim();
    if (!id || !name) return;
    await updateCategory(props.guildId, id, { name });
    setEditingId(null);
    setEditingName("");
  };

  const cancelEditing = () => {
    setEditingId(null);
    setEditingName("");
  };

  const handleDelete = async (id: string) => {
    const success = await deleteCategory(props.guildId, id);
    if (success) setDeletingId(null);
  };

  const renderCategoryRow = (categoryId: string, name: string, isSubcat: boolean, catType: string = "mixed") => {
    const isEditing = () => editingId() === categoryId;
    const isDeleting = () => deletingId() === categoryId;

    return (
      <div
        class="flex items-center gap-2 px-3 py-2 rounded-lg hover:bg-white/5 group"
        classList={{ "ml-6": isSubcat }}
      >
        <Show when={!isSubcat}>
          <ChevronRight class="w-3.5 h-3.5 text-text-muted shrink-0" />
        </Show>

        <Show
          when={!isEditing()}
          fallback={
            <div class="flex-1 flex items-center gap-2">
              <input
                type="text"
                value={editingName()}
                onInput={(e) => setEditingName(e.currentTarget.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") handleRename();
                  if (e.key === "Escape") cancelEditing();
                }}
                class="flex-1 px-2 py-1 bg-surface-layer2 rounded text-sm text-text-primary border border-white/10 outline-none focus:ring-1 focus:ring-accent-primary"
                ref={(el) => setTimeout(() => el.focus(), 0)}
              />
              <button
                onClick={handleRename}
                class="p-1 text-accent-success hover:bg-white/10 rounded"
                title="Save"
              >
                <Check class="w-4 h-4" />
              </button>
              <button
                onClick={cancelEditing}
                class="p-1 text-text-secondary hover:bg-white/10 rounded"
                title="Cancel"
              >
                <X class="w-4 h-4" />
              </button>
            </div>
          }
        >
          <span class="flex-1 text-sm text-text-primary truncate">{name}</span>
          <Show when={catType !== "mixed"}>
            <span class="flex items-center gap-1 text-[11px] text-text-muted px-1.5 py-0.5 rounded bg-white/5 shrink-0">
              {typeIcon(catType)} {typeLabel(catType)}
            </span>
          </Show>
          <Show when={!isDeleting()}>
            <div class="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
              <Show when={!isSubcat}>
                <button
                  onClick={() => {
                    setAddingSubcategoryFor(addingSubcategoryFor() === categoryId ? null : categoryId);
                    setSubCategoryName("");
                  }}
                  class="p-1 text-text-secondary hover:text-accent-primary hover:bg-white/10 rounded transition-colors"
                  title="Add Subcategory"
                >
                  <Plus class="w-3.5 h-3.5" />
                </button>
              </Show>
              <button
                onClick={() => startEditing(categoryId, name)}
                class="p-1 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded transition-colors"
                title="Rename"
              >
                <Pencil class="w-3.5 h-3.5" />
              </button>
              <button
                onClick={() => setDeletingId(categoryId)}
                class="p-1 text-text-secondary hover:text-accent-danger hover:bg-white/10 rounded transition-colors"
                title="Delete"
              >
                <Trash2 class="w-3.5 h-3.5" />
              </button>
            </div>
          </Show>
        </Show>

        {/* Delete confirmation inline */}
        <Show when={isDeleting()}>
          <div class="flex items-center gap-2">
            <span class="text-xs text-accent-danger">Delete?</span>
            <button
              onClick={() => handleDelete(categoryId)}
              class="px-2 py-0.5 text-xs bg-accent-danger text-white rounded hover:opacity-80"
            >
              Yes
            </button>
            <button
              onClick={() => setDeletingId(null)}
              class="px-2 py-0.5 text-xs bg-white/10 text-text-primary rounded hover:bg-white/20"
            >
              No
            </button>
          </div>
        </Show>
      </div>
    );
  };

  return (
    <div class="p-6 space-y-6">
      <div>
        <h3 class="text-lg font-semibold text-text-primary mb-1">Categories</h3>
        <p class="text-sm text-text-secondary">
          Organize channels into categories. Drag channels in the sidebar to move them between categories.
        </p>
      </div>

      {/* Create new category */}
      <div class="flex gap-2 items-center">
        <input
          type="text"
          placeholder="New category name"
          value={newCategoryName()}
          onInput={(e) => setNewCategoryName(e.currentTarget.value)}
          onKeyDown={(e) => { if (e.key === "Enter") handleCreate(); }}
          class="flex-1 px-3 py-2 bg-surface-layer2 rounded-lg text-sm text-text-primary border border-white/10 outline-none focus:ring-1 focus:ring-accent-primary placeholder-text-secondary"
          disabled={isCreating()}
        />
        <div class="flex rounded-lg border border-white/10 overflow-hidden shrink-0">
          {(["mixed", "text", "voice"] as const).map((t) => (
            <button
              onClick={() => setNewCategoryType(t)}
              class="px-2 py-2 text-xs transition-colors"
              classList={{
                "bg-accent-primary text-on-accent": newCategoryType() === t,
                "bg-surface-layer2 text-text-secondary hover:text-text-primary": newCategoryType() !== t,
              }}
              title={typeLabel(t)}
            >
              {typeIcon(t)}
            </button>
          ))}
        </div>
        <button
          onClick={handleCreate}
          disabled={!newCategoryName().trim() || isCreating()}
          class="px-4 py-2 bg-accent-primary text-on-accent rounded-lg text-sm font-medium hover:opacity-90 transition-opacity disabled:opacity-50 disabled:cursor-not-allowed shrink-0"
        >
          Add
        </button>
      </div>

      {/* Categories list */}
      <Show
        when={!categoriesState.isLoading}
        fallback={<p class="text-sm text-text-secondary">Loading categories...</p>}
      >
        <Show
          when={topLevel().length > 0}
          fallback={
            <p class="text-sm text-text-secondary py-4 text-center">
              No categories yet. Create one above to organize your channels.
            </p>
          }
        >
          <div class="space-y-1 bg-surface-layer1 rounded-lg border border-white/5 py-2">
            <For each={topLevel()}>
              {(category) => {
                const subs = () => getSubcategories(props.guildId, category.id);
                return (
                  <>
                    {renderCategoryRow(category.id, category.name, false, category.category_type)}

                    {/* Subcategories */}
                    <For each={subs()}>
                      {(sub) => renderCategoryRow(sub.id, sub.name, true, sub.category_type)}
                    </For>

                    {/* Add subcategory inline form */}
                    <Show when={addingSubcategoryFor() === category.id}>
                      <div class="flex items-center gap-2 ml-6 px-3 py-1.5">
                        <input
                          type="text"
                          placeholder="Subcategory name"
                          value={subCategoryName()}
                          onInput={(e) => setSubCategoryName(e.currentTarget.value)}
                          onKeyDown={(e) => {
                            if (e.key === "Enter") handleCreateSubcategory(category.id);
                            if (e.key === "Escape") setAddingSubcategoryFor(null);
                          }}
                          class="flex-1 px-2 py-1 bg-surface-layer2 rounded text-sm text-text-primary border border-white/10 outline-none focus:ring-1 focus:ring-accent-primary placeholder-text-secondary"
                          disabled={isCreating()}
                          ref={(el) => setTimeout(() => el.focus(), 0)}
                        />
                        <button
                          onClick={() => handleCreateSubcategory(category.id)}
                          disabled={!subCategoryName().trim() || isCreating()}
                          class="p-1 text-accent-success hover:bg-white/10 rounded disabled:opacity-50"
                          title="Add"
                        >
                          <Check class="w-4 h-4" />
                        </button>
                        <button
                          onClick={() => setAddingSubcategoryFor(null)}
                          class="p-1 text-text-secondary hover:bg-white/10 rounded"
                          title="Cancel"
                        >
                          <X class="w-4 h-4" />
                        </button>
                      </div>
                    </Show>
                  </>
                );
              }}
            </For>
          </div>
        </Show>
      </Show>
    </div>
  );
};

export default CategoriesTab;
