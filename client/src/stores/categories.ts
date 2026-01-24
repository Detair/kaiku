/**
 * Categories Store
 *
 * Manages channel category state including collapse states for the UI.
 */

import { createStore } from "solid-js/store";
import type { ChannelCategory } from "@/lib/types";
import * as tauri from "@/lib/tauri";

// ============================================================================
// State Interface
// ============================================================================

interface CategoriesState {
  /** Categories indexed by guild ID */
  categories: Record<string, ChannelCategory[]>;
  /** Collapse state indexed by category ID (local UI state) */
  collapseState: Record<string, boolean>;
  /** Loading state for categories fetch */
  isLoading: boolean;
  /** Error message if categories fetch failed */
  error: string | null;
}

// ============================================================================
// Store
// ============================================================================

const [categoriesState, setCategoriesState] = createStore<CategoriesState>({
  categories: {},
  collapseState: {},
  isLoading: false,
  error: null,
});

// ============================================================================
// Actions
// ============================================================================

/**
 * Load categories for a specific guild from the server.
 */
export async function loadGuildCategories(guildId: string): Promise<void> {
  setCategoriesState({ isLoading: true, error: null });

  try {
    const categories = await tauri.getGuildCategories(guildId);
    setGuildCategories(guildId, categories);
    setCategoriesState({ isLoading: false });
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to load guild categories:", error);
    setCategoriesState({ isLoading: false, error });
  }
}

/**
 * Set categories for a guild (used by loadGuildCategories and for initial data).
 */
export function setGuildCategories(
  guildId: string,
  categories: ChannelCategory[]
): void {
  setCategoriesState("categories", guildId, categories);

  // Initialize collapse state from the server data if not already set locally
  for (const cat of categories) {
    if (categoriesState.collapseState[cat.id] === undefined) {
      setCategoriesState("collapseState", cat.id, cat.collapsed);
    }
  }
}

/**
 * Toggle the collapse state of a category.
 */
export function toggleCategoryCollapse(categoryId: string): void {
  const currentState = categoriesState.collapseState[categoryId] ?? false;
  setCategoriesState("collapseState", categoryId, !currentState);

  // TODO: Persist to server (optional, could be local-only)
  // tauri.updateCategoryCollapse(categoryId, !currentState).catch(console.error);
}

/**
 * Set the collapse state of a category directly.
 */
export function setCategoryCollapse(
  categoryId: string,
  collapsed: boolean
): void {
  setCategoriesState("collapseState", categoryId, collapsed);
}

/**
 * Check if a category is collapsed.
 */
export function isCategoryCollapsed(categoryId: string): boolean {
  return categoriesState.collapseState[categoryId] ?? false;
}

/**
 * Get all categories for a guild.
 */
export function getGuildCategories(guildId: string): ChannelCategory[] {
  return categoriesState.categories[guildId] ?? [];
}

/**
 * Get top-level categories (no parent) for a guild, sorted by position.
 */
export function getTopLevelCategories(guildId: string): ChannelCategory[] {
  const categories = categoriesState.categories[guildId] ?? [];
  return categories
    .filter((c) => c.parent_id === null)
    .sort((a, b) => a.position - b.position);
}

/**
 * Get subcategories for a parent category, sorted by position.
 */
export function getSubcategories(
  guildId: string,
  parentId: string
): ChannelCategory[] {
  const categories = categoriesState.categories[guildId] ?? [];
  return categories
    .filter((c) => c.parent_id === parentId)
    .sort((a, b) => a.position - b.position);
}

/**
 * Get a category by ID.
 */
export function getCategory(categoryId: string): ChannelCategory | undefined {
  for (const guildCategories of Object.values(categoriesState.categories)) {
    const found = guildCategories.find((c) => c.id === categoryId);
    if (found) return found;
  }
  return undefined;
}

/**
 * Clear categories for a guild.
 */
export function clearGuildCategories(guildId: string): void {
  setCategoriesState("categories", guildId, []);
}

/**
 * Clear all collapse state (useful on logout).
 */
export function clearCollapseState(): void {
  setCategoriesState("collapseState", {});
}

/**
 * Reorder categories for a guild.
 * Updates local state optimistically, then persists to server.
 *
 * @param guildId - The guild ID
 * @param categoryId - The category being moved
 * @param targetCategoryId - The target category (to drop before/after/inside)
 * @param position - 'before', 'after', or 'inside' (for nesting)
 */
export async function reorderCategories(
  guildId: string,
  categoryId: string,
  targetCategoryId: string,
  position: "before" | "after" | "inside"
): Promise<void> {
  const categories = categoriesState.categories[guildId] ?? [];
  const category = categories.find((c) => c.id === categoryId);
  const targetCategory = categories.find((c) => c.id === targetCategoryId);

  if (!category || !targetCategory) {
    console.error("Category not found for reorder");
    return;
  }

  // Check 2-level nesting constraint
  if (position === "inside" && targetCategory.parent_id !== null) {
    console.warn("Cannot nest category more than 2 levels deep");
    return;
  }

  // Calculate new positions and parent_id
  let newParentId: string | null;
  let insertPosition: number;

  if (position === "inside") {
    // Moving into a category as a subcategory
    newParentId = targetCategoryId;
    const subcategories = categories.filter((c) => c.parent_id === targetCategoryId);
    insertPosition = subcategories.length;
  } else {
    // Moving before/after - same parent as target
    newParentId = targetCategory.parent_id;

    // Get siblings (categories at the same level)
    const siblings = categories
      .filter((c) => c.parent_id === newParentId)
      .sort((a, b) => a.position - b.position);

    const targetIndex = siblings.findIndex((c) => c.id === targetCategoryId);
    insertPosition = position === "before" ? targetIndex : targetIndex + 1;
  }

  // Build the reorder request
  // We need to recalculate positions for all affected categories
  const categoriesToUpdate: Array<{ id: string; position: number; parentId: string | null }> = [];

  // Get all categories at the same level as the target
  const affectedCategories = categories
    .filter((c) => c.parent_id === newParentId && c.id !== categoryId)
    .sort((a, b) => a.position - b.position);

  // Insert the moved category at the new position
  affectedCategories.splice(insertPosition, 0, { ...category, parent_id: newParentId });

  // Assign new positions
  affectedCategories.forEach((cat, index) => {
    categoriesToUpdate.push({
      id: cat.id,
      position: index,
      parentId: cat.id === categoryId ? newParentId : cat.parent_id,
    });
  });

  // If the category was moved to a different parent, also update categories at the old level
  if (category.parent_id !== newParentId) {
    const oldSiblings = categories
      .filter((c) => c.parent_id === category.parent_id && c.id !== categoryId)
      .sort((a, b) => a.position - b.position);

    oldSiblings.forEach((cat, index) => {
      // Only add if not already in the update list
      if (!categoriesToUpdate.find((u) => u.id === cat.id)) {
        categoriesToUpdate.push({
          id: cat.id,
          position: index,
          parentId: cat.parent_id,
        });
      }
    });
  }

  // Optimistic update
  const updatedCategories = categories.map((cat) => {
    const update = categoriesToUpdate.find((u) => u.id === cat.id);
    if (update) {
      return {
        ...cat,
        position: update.position,
        parent_id: update.parentId,
      };
    }
    return cat;
  });

  setCategoriesState("categories", guildId, updatedCategories);

  // Persist to server
  try {
    await tauri.reorderGuildCategories(guildId, categoriesToUpdate);
  } catch (err) {
    console.error("Failed to reorder categories:", err);
    // Revert on error
    await loadGuildCategories(guildId);
  }
}

/**
 * Check if a category is a subcategory (has a parent).
 */
export function isSubcategory(categoryId: string): boolean {
  const category = getCategory(categoryId);
  return category?.parent_id !== null;
}

// ============================================================================
// Export
// ============================================================================

export { categoriesState };
