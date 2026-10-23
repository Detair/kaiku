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
import { httpRequest } from "./common";

// ============================================================================
// Platform pages
// ============================================================================

/**
 * List all platform pages.
 */
export async function listPlatformPages(): Promise<PageListItem[]> {
  return httpRequest<PageListItem[]>("GET", "/api/pages");
}

/**
 * Get a platform page by slug.
 */
export async function getPlatformPage(slug: string): Promise<Page> {
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
  await httpRequest<void>("DELETE", `/api/pages/${pageId}`);
}

/**
 * Reorder platform pages (admin only).
 */
export async function reorderPlatformPages(pageIds: string[]): Promise<void> {
  await httpRequest<void>("POST", "/api/pages/reorder", { page_ids: pageIds });
}

// ============================================================================
// Guild pages
// ============================================================================

/**
 * List guild pages.
 */
export async function listGuildPages(guildId: string): Promise<PageListItem[]> {
  return httpRequest<PageListItem[]>("GET", `/api/guilds/${guildId}/pages`);
}

/**
 * Get a guild page by slug.
 */
export async function getGuildPage(
  guildId: string,
  slug: string,
): Promise<Page> {
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
  await httpRequest<void>("DELETE", `/api/guilds/${guildId}/pages/${pageId}`);
}

/**
 * Reorder guild pages.
 */
export async function reorderGuildPages(
  guildId: string,
  pageIds: string[],
): Promise<void> {
  await httpRequest<void>("POST", `/api/guilds/${guildId}/pages/reorder`, {
    page_ids: pageIds,
  });
}

/**
 * Accept a page.
 */
export async function acceptPage(pageId: string): Promise<void> {
  await httpRequest<void>("POST", `/api/pages/${pageId}/accept`);
}

/**
 * Get pages pending acceptance.
 */
export async function getPendingAcceptance(): Promise<PageListItem[]> {
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
  await httpRequest<void>(
    "POST",
    `/api/guilds/${guildId}/page-categories/reorder`,
    {
      category_ids: categoryIds,
    },
  );
}
