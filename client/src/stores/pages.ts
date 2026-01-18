/**
 * Pages Store
 *
 * Manages information pages state for platform and guild pages.
 */

import { createStore } from "solid-js/store";
import type { Page, PageListItem } from "@/lib/types";
import * as tauri from "@/lib/tauri";

// ============================================================================
// Helper Functions
// ============================================================================

/** Sort pages by position ascending. */
function sortByPosition<T extends { position: number }>(pages: T[]): T[] {
  return [...pages].sort((a, b) => a.position - b.position);
}

/** Convert Page to PageListItem. */
function toListItem(page: Page): PageListItem {
  return {
    id: page.id,
    guild_id: page.guild_id,
    title: page.title,
    slug: page.slug,
    position: page.position,
    requires_acceptance: page.requires_acceptance,
    updated_at: page.updated_at,
  };
}

// Pages state interface
interface PagesState {
  platformPages: PageListItem[];
  guildPages: Record<string, PageListItem[]>;
  currentPage: Page | null;
  pendingAcceptance: PageListItem[];
  isLoading: boolean;
  isPlatformLoading: boolean;
  error: string | null;
}

// Create the store
const [pagesState, setPagesState] = createStore<PagesState>({
  platformPages: [],
  guildPages: {},
  currentPage: null,
  pendingAcceptance: [],
  isLoading: false,
  isPlatformLoading: false,
  error: null,
});

// ============================================================================
// Platform Pages Actions
// ============================================================================

/**
 * Load platform pages.
 */
export async function loadPlatformPages(): Promise<void> {
  setPagesState({ isPlatformLoading: true, error: null });

  try {
    const pages = await tauri.listPlatformPages();
    setPagesState({
      platformPages: sortByPosition(pages),
      isPlatformLoading: false,
      error: null,
    });
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to load platform pages:", error);
    setPagesState({ isPlatformLoading: false, error });
  }
}

/**
 * Get a platform page by slug.
 */
export async function loadPlatformPage(slug: string): Promise<Page | null> {
  setPagesState({ isLoading: true, error: null });

  try {
    const page = await tauri.getPlatformPage(slug);
    setPagesState({
      currentPage: page,
      isLoading: false,
      error: null,
    });
    return page;
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to load platform page:", error);
    setPagesState({ isLoading: false, error });
    return null;
  }
}

/**
 * Create a platform page.
 */
export async function createPlatformPage(
  title: string,
  content: string,
  slug?: string,
  requiresAcceptance?: boolean
): Promise<Page | null> {
  setPagesState({ isLoading: true, error: null });

  try {
    const page = await tauri.createPlatformPage(title, content, slug, requiresAcceptance);
    setPagesState("platformPages", (prev) =>
      sortByPosition([...prev, toListItem(page)])
    );
    setPagesState({ isLoading: false, error: null });
    return page;
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to create platform page:", error);
    setPagesState({ isLoading: false, error });
    return null;
  }
}

/**
 * Update a platform page.
 */
export async function updatePlatformPage(
  pageId: string,
  title?: string,
  slug?: string,
  content?: string,
  requiresAcceptance?: boolean
): Promise<Page | null> {
  setPagesState({ isLoading: true, error: null });

  try {
    const page = await tauri.updatePlatformPage(pageId, title, slug, content, requiresAcceptance);
    setPagesState("platformPages", (prev) =>
      prev.map((p) =>
        p.id === pageId
          ? {
              id: page.id,
              guild_id: page.guild_id,
              title: page.title,
              slug: page.slug,
              position: page.position,
              requires_acceptance: page.requires_acceptance,
              updated_at: page.updated_at,
            }
          : p
      )
    );
    if (pagesState.currentPage?.id === pageId) {
      setPagesState({ currentPage: page });
    }
    setPagesState({ isLoading: false, error: null });
    return page;
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to update platform page:", error);
    setPagesState({ isLoading: false, error });
    return null;
  }
}

/**
 * Delete a platform page.
 */
export async function deletePlatformPage(pageId: string): Promise<boolean> {
  setPagesState({ isLoading: true, error: null });

  try {
    await tauri.deletePlatformPage(pageId);
    setPagesState("platformPages", (prev) => prev.filter((p) => p.id !== pageId));
    if (pagesState.currentPage?.id === pageId) {
      setPagesState({ currentPage: null });
    }
    setPagesState({ isLoading: false, error: null });
    return true;
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to delete platform page:", error);
    setPagesState({ isLoading: false, error });
    return false;
  }
}

/**
 * Reorder platform pages.
 */
export async function reorderPlatformPages(pageIds: string[]): Promise<boolean> {
  try {
    await tauri.reorderPlatformPages(pageIds);
    // Update local state to reflect new order
    setPagesState("platformPages", (prev) =>
      pageIds
        .map((id, index) => {
          const page = prev.find((p) => p.id === id);
          return page ? { ...page, position: index } : null;
        })
        .filter((p): p is PageListItem => p !== null)
    );
    return true;
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to reorder platform pages:", error);
    setPagesState({ error });
    return false;
  }
}

// ============================================================================
// Guild Pages Actions
// ============================================================================

/**
 * Load pages for a guild.
 */
export async function loadGuildPages(guildId: string): Promise<void> {
  setPagesState({ isLoading: true, error: null });

  try {
    const pages = await tauri.listGuildPages(guildId);
    setPagesState("guildPages", guildId, sortByPosition(pages));
    setPagesState({ isLoading: false, error: null });
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to load guild pages:", error);
    setPagesState({ isLoading: false, error });
  }
}

/**
 * Get a guild page by slug.
 */
export async function loadGuildPage(guildId: string, slug: string): Promise<Page | null> {
  setPagesState({ isLoading: true, error: null });

  try {
    const page = await tauri.getGuildPage(guildId, slug);
    setPagesState({
      currentPage: page,
      isLoading: false,
      error: null,
    });
    return page;
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to load guild page:", error);
    setPagesState({ isLoading: false, error });
    return null;
  }
}

/**
 * Create a guild page.
 */
export async function createGuildPage(
  guildId: string,
  title: string,
  content: string,
  slug?: string,
  requiresAcceptance?: boolean
): Promise<Page | null> {
  setPagesState({ isLoading: true, error: null });

  try {
    const page = await tauri.createGuildPage(guildId, title, content, slug, requiresAcceptance);
    setPagesState("guildPages", guildId, (prev) =>
      sortByPosition([...(prev || []), toListItem(page)])
    );
    setPagesState({ isLoading: false, error: null });
    return page;
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to create guild page:", error);
    setPagesState({ isLoading: false, error });
    return null;
  }
}

/**
 * Update a guild page.
 */
export async function updateGuildPage(
  guildId: string,
  pageId: string,
  title?: string,
  slug?: string,
  content?: string,
  requiresAcceptance?: boolean
): Promise<Page | null> {
  setPagesState({ isLoading: true, error: null });

  try {
    const page = await tauri.updateGuildPage(guildId, pageId, title, slug, content, requiresAcceptance);
    setPagesState("guildPages", guildId, (prev) =>
      (prev || []).map((p) =>
        p.id === pageId
          ? {
              id: page.id,
              guild_id: page.guild_id,
              title: page.title,
              slug: page.slug,
              position: page.position,
              requires_acceptance: page.requires_acceptance,
              updated_at: page.updated_at,
            }
          : p
      )
    );
    if (pagesState.currentPage?.id === pageId) {
      setPagesState({ currentPage: page });
    }
    setPagesState({ isLoading: false, error: null });
    return page;
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to update guild page:", error);
    setPagesState({ isLoading: false, error });
    return null;
  }
}

/**
 * Delete a guild page.
 */
export async function deleteGuildPage(guildId: string, pageId: string): Promise<boolean> {
  setPagesState({ isLoading: true, error: null });

  try {
    await tauri.deleteGuildPage(guildId, pageId);
    setPagesState("guildPages", guildId, (prev) =>
      (prev || []).filter((p) => p.id !== pageId)
    );
    if (pagesState.currentPage?.id === pageId) {
      setPagesState({ currentPage: null });
    }
    setPagesState({ isLoading: false, error: null });
    return true;
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to delete guild page:", error);
    setPagesState({ isLoading: false, error });
    return false;
  }
}

/**
 * Reorder guild pages.
 */
export async function reorderGuildPages(guildId: string, pageIds: string[]): Promise<boolean> {
  try {
    await tauri.reorderGuildPages(guildId, pageIds);
    setPagesState("guildPages", guildId, (prev) =>
      pageIds
        .map((id, index) => {
          const page = (prev || []).find((p) => p.id === id);
          return page ? { ...page, position: index } : null;
        })
        .filter((p): p is PageListItem => p !== null)
    );
    return true;
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to reorder guild pages:", error);
    setPagesState({ error });
    return false;
  }
}

// ============================================================================
// Acceptance Actions
// ============================================================================

/**
 * Load pages pending acceptance.
 */
export async function loadPendingAcceptance(): Promise<void> {
  try {
    const pages = await tauri.getPendingAcceptance();
    setPagesState({ pendingAcceptance: pages });
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to load pending acceptance:", error);
    setPagesState({ error });
  }
}

/**
 * Accept a page.
 */
export async function acceptPage(pageId: string): Promise<boolean> {
  try {
    await tauri.acceptPage(pageId);
    setPagesState("pendingAcceptance", (prev) =>
      prev.filter((p) => p.id !== pageId)
    );
    return true;
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to accept page:", error);
    setPagesState({ error });
    return false;
  }
}

// ============================================================================
// Utility Functions
// ============================================================================

/**
 * Clear current page.
 */
export function clearCurrentPage(): void {
  setPagesState({ currentPage: null });
}

/**
 * Get guild pages from state.
 */
export function getGuildPagesFromState(guildId: string): PageListItem[] {
  return pagesState.guildPages[guildId] || [];
}

/**
 * Check if there are pending platform pages requiring acceptance.
 */
export function hasPendingPlatformPages(): boolean {
  return pagesState.pendingAcceptance.some((p) => p.guild_id === null);
}

/**
 * Check if there are pending guild pages requiring acceptance.
 */
export function hasPendingGuildPages(guildId: string): boolean {
  return pagesState.pendingAcceptance.some((p) => p.guild_id === guildId);
}

// Export the store for reading
export { pagesState };
