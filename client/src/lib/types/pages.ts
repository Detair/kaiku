/**
 * Page, page revision, and page category types.
 */

export interface Page {
  id: string;
  guild_id: string | null;
  title: string;
  slug: string;
  content: string;
  content_hash: string;
  position: number;
  requires_acceptance: boolean;
  category_id: string | null;
  created_by: string;
  updated_by: string;
  created_at: string;
  updated_at: string;
  deleted_at: string | null;
}

export interface PageListItem {
  id: string;
  guild_id: string | null;
  title: string;
  slug: string;
  position: number;
  requires_acceptance: boolean;
  category_id: string | null;
  updated_at: string;
}

export interface CreatePageRequest {
  title: string;
  slug?: string;
  content: string;
  requires_acceptance?: boolean;
  category_id?: string;
}

export interface UpdatePageRequest {
  title?: string;
  slug?: string;
  content?: string;
  requires_acceptance?: boolean;
  category_id?: string | null;
}

export interface PageRevision {
  id: string;
  page_id: string;
  revision_number: number;
  content: string | null;
  content_hash: string | null;
  title: string | null;
  created_by: string | null;
  created_at: string;
}

export interface RevisionListItem {
  id: string;
  page_id: string;
  revision_number: number;
  content_hash: string | null;
  title: string | null;
  created_by: string | null;
  created_at: string;
}

export interface PageCategory {
  id: string;
  guild_id: string;
  name: string;
  position: number;
  created_at: string;
}
