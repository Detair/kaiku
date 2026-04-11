/**
 * Platform pages, guild pages, revisions, and page categories.
 */

import type {
  Page,
  PageCategory,
  PageListItem,
  PageRevision,
  RevisionListItem,
} from "../types";
import { httpRequest, isTauri } from "./common";

// ============================================================================
// Platform pages
// ============================================================================

/**
 * List all platform pages.
 */
export async function listPlatformPages(): Promise<PageListItem[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("list_platform_pages");
  }

  return httpRequest<PageListItem[]>("GET", "/api/pages");
}

/**
 * Get a platform page by slug.
 */
export async function getPlatformPage(slug: string): Promise<Page> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_platform_page", { slug });
  }

  return httpRequest<Page>("GET", `/api/pages/by-slug/${slug}`);
}

/**
 * Create a platform page (admin only).
 */
export async function createPlatformPage(
  title: string,
  content: string,
  slug?: string,
  requiresAcceptance?: boolean,
): Promise<Page> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("create_platform_page", {
      title,
      content,
      slug,
      requiresAcceptance,
    });
  }

  return httpRequest<Page>("POST", "/api/pages", {
    title,
    content,
    slug,
    requires_acceptance: requiresAcceptance,
  });
}

/**
 * Update a platform page (admin only).
 */
export async function updatePlatformPage(
  pageId: string,
  title?: string,
  slug?: string,
  content?: string,
  requiresAcceptance?: boolean,
): Promise<Page> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("update_platform_page", {
      pageId,
      title,
      slug,
      content,
      requiresAcceptance,
    });
  }

  return httpRequest<Page>("PATCH", `/api/pages/${pageId}`, {
    title,
    slug,
    content,
    requires_acceptance: requiresAcceptance,
  });
}

/**
 * Delete a platform page (admin only).
 */
export async function deletePlatformPage(pageId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("delete_platform_page", { pageId });
  }

  await httpRequest<void>("DELETE", `/api/pages/${pageId}`);
}

/**
 * Reorder platform pages (admin only).
 */
export async function reorderPlatformPages(pageIds: string[]): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("reorder_platform_pages", { pageIds });
  }

  await httpRequest<void>("POST", "/api/pages/reorder", { page_ids: pageIds });
}

// ============================================================================
// Guild pages
// ============================================================================

/**
 * List guild pages.
 */
export async function listGuildPages(guildId: string): Promise<PageListItem[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("list_guild_pages", { guildId });
  }

  return httpRequest<PageListItem[]>("GET", `/api/guilds/${guildId}/pages`);
}

/**
 * Get a guild page by slug.
 */
export async function getGuildPage(
  guildId: string,
  slug: string,
): Promise<Page> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_guild_page", { guildId, slug });
  }

  return httpRequest<Page>(
    "GET",
    `/api/guilds/${guildId}/pages/by-slug/${slug}`,
  );
}

/**
 * Create a guild page.
 */
export async function createGuildPage(
  guildId: string,
  title: string,
  content: string,
  slug?: string,
  requiresAcceptance?: boolean,
  categoryId?: string,
): Promise<Page> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("create_guild_page", {
      guildId,
      title,
      content,
      slug,
      requiresAcceptance,
      categoryId,
    });
  }

  return httpRequest<Page>("POST", `/api/guilds/${guildId}/pages`, {
    title,
    content,
    slug,
    requires_acceptance: requiresAcceptance,
    category_id: categoryId,
  });
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
  requiresAcceptance?: boolean,
  categoryId?: string | null,
): Promise<Page> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("update_guild_page", {
      guildId,
      pageId,
      title,
      slug,
      content,
      requiresAcceptance,
      categoryId,
    });
  }

  // Build body — only include category_id if explicitly provided
  const body: Record<string, unknown> = {
    title,
    slug,
    content,
    requires_acceptance: requiresAcceptance,
  };
  if (categoryId !== undefined) {
    body.category_id = categoryId;
  }

  return httpRequest<Page>(
    "PATCH",
    `/api/guilds/${guildId}/pages/${pageId}`,
    body,
  );
}

/**
 * Delete a guild page.
 */
export async function deleteGuildPage(
  guildId: string,
  pageId: string,
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("delete_guild_page", { guildId, pageId });
  }

  await httpRequest<void>("DELETE", `/api/guilds/${guildId}/pages/${pageId}`);
}

/**
 * Reorder guild pages.
 */
export async function reorderGuildPages(
  guildId: string,
  pageIds: string[],
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("reorder_guild_pages", { guildId, pageIds });
  }

  await httpRequest<void>("POST", `/api/guilds/${guildId}/pages/reorder`, {
    page_ids: pageIds,
  });
}

/**
 * Accept a page.
 */
export async function acceptPage(pageId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("accept_page", { pageId });
  }

  await httpRequest<void>("POST", `/api/pages/${pageId}/accept`);
}

/**
 * Get pages pending acceptance.
 */
export async function getPendingAcceptance(): Promise<PageListItem[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_pending_acceptance");
  }

  return httpRequest<PageListItem[]>("GET", "/api/pages/pending-acceptance");
}

// ============================================================================
// Page Revisions
// ============================================================================

/**
 * List all revisions for a guild page.
 */
export async function listPageRevisions(
  guildId: string,
  pageId: string,
): Promise<RevisionListItem[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("list_page_revisions", { guildId, pageId });
  }

  return httpRequest<RevisionListItem[]>(
    "GET",
    `/api/guilds/${guildId}/pages/${pageId}/revisions`,
  );
}

/**
 * Get a specific revision of a guild page.
 */
export async function getPageRevision(
  guildId: string,
  pageId: string,
  revisionNumber: number,
): Promise<PageRevision> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_page_revision", { guildId, pageId, revisionNumber });
  }

  return httpRequest<PageRevision>(
    "GET",
    `/api/guilds/${guildId}/pages/${pageId}/revisions/${revisionNumber}`,
  );
}

/**
 * Restore a guild page to a specific revision.
 */
export async function restorePageRevision(
  guildId: string,
  pageId: string,
  revisionNumber: number,
): Promise<Page> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("restore_page_revision", { guildId, pageId, revisionNumber });
  }

  return httpRequest<Page>(
    "POST",
    `/api/guilds/${guildId}/pages/${pageId}/revisions/${revisionNumber}/restore`,
  );
}

// ============================================================================
// Page Categories
// ============================================================================

/**
 * List all page categories for a guild.
 */
export async function listPageCategories(
  guildId: string,
): Promise<PageCategory[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("list_page_categories", { guildId });
  }

  return httpRequest<PageCategory[]>(
    "GET",
    `/api/guilds/${guildId}/page-categories`,
  );
}

/**
 * Create a page category in a guild.
 */
export async function createPageCategory(
  guildId: string,
  name: string,
): Promise<PageCategory> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("create_page_category", { guildId, name });
  }

  return httpRequest<PageCategory>(
    "POST",
    `/api/guilds/${guildId}/page-categories`,
    {
      name,
    },
  );
}

/**
 * Update a page category in a guild.
 */
export async function updatePageCategory(
  guildId: string,
  categoryId: string,
  name: string,
): Promise<PageCategory> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("update_page_category", { guildId, categoryId, name });
  }

  return httpRequest<PageCategory>(
    "PATCH",
    `/api/guilds/${guildId}/page-categories/${categoryId}`,
    { name },
  );
}

/**
 * Delete a page category from a guild.
 */
export async function deletePageCategory(
  guildId: string,
  categoryId: string,
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("delete_page_category", { guildId, categoryId });
  }

  await httpRequest<void>(
    "DELETE",
    `/api/guilds/${guildId}/page-categories/${categoryId}`,
  );
}

/**
 * Reorder page categories in a guild.
 */
export async function reorderPageCategories(
  guildId: string,
  categoryIds: string[],
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("reorder_page_categories", { guildId, categoryIds });
  }

  await httpRequest<void>(
    "POST",
    `/api/guilds/${guildId}/page-categories/reorder`,
    {
      category_ids: categoryIds,
    },
  );
}
