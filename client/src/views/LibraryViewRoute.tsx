/**
 * Library View Route
 *
 * Route component for the guild library (browsable page catalog).
 * URL: /guilds/:guildId/library
 */

import { Component, Show, createSignal, onMount } from "solid-js";
import { useParams, useNavigate } from "@solidjs/router";
import { LibraryCatalog, CategoryManager } from "@/components/pages";
import type { PageListItem } from "@/lib/types";
import { authState } from "@/stores/auth";
import { isGuildOwner } from "@/stores/guilds";
import { memberHasPermission } from "@/stores/permissions";
import { PermissionBits } from "@/lib/permissionConstants";
import {
  loadGuildPages,
  loadGuildCategories,
  createCategory,
  updateCategory,
  deleteCategory,
  getGuildPagesFromState,
  getGuildCategoriesFromState,
  pagesState,
} from "@/stores/pages";

const LibraryViewRoute: Component = () => {
  const params = useParams<{ guildId: string }>();
  const navigate = useNavigate();
  const [showCategoryManager, setShowCategoryManager] = createSignal(false);

  const canManage = () => {
    const userId = authState.user?.id;
    const guildId = params.guildId;
    if (!userId || !guildId) return false;
    const isOwner = isGuildOwner(guildId, userId);
    return (
      isOwner ||
      memberHasPermission(guildId, userId, isOwner, PermissionBits.MANAGE_PAGES)
    );
  };

  onMount(() => {
    if (params.guildId) {
      loadGuildPages(params.guildId);
      loadGuildCategories(params.guildId);
    }
  });

  const handlePageClick = (page: PageListItem) => {
    navigate(`/guilds/${params.guildId}/pages/${page.slug}`);
  };

  const handleNewPage = (categoryId?: string) => {
    const base = `/guilds/${params.guildId}/pages/new`;
    navigate(categoryId ? `${base}?category=${categoryId}` : base);
  };

  const handleCreateCategory = async (name: string) => {
    await createCategory(params.guildId, name);
  };

  const handleUpdateCategory = async (categoryId: string, name: string) => {
    await updateCategory(params.guildId, categoryId, name);
  };

  const handleDeleteCategory = async (categoryId: string) => {
    await deleteCategory(params.guildId, categoryId);
  };

  return (
    <div class="h-screen bg-zinc-900 flex flex-col">
      <Show
        when={!pagesState.isLoading}
        fallback={
          <div class="flex-1 flex items-center justify-center">
            <div class="text-zinc-400">Loading library...</div>
          </div>
        }
      >
        <LibraryCatalog
          guildId={params.guildId}
          pages={getGuildPagesFromState(params.guildId)}
          categories={getGuildCategoriesFromState(params.guildId)}
          canManage={canManage()}
          onPageClick={handlePageClick}
          onNewPage={handleNewPage}
          onManageCategories={() => setShowCategoryManager(true)}
        />
      </Show>

      <Show when={showCategoryManager()}>
        <CategoryManager
          guildId={params.guildId}
          categories={getGuildCategoriesFromState(params.guildId)}
          onClose={() => setShowCategoryManager(false)}
          onCreateCategory={handleCreateCategory}
          onUpdateCategory={handleUpdateCategory}
          onDeleteCategory={handleDeleteCategory}
        />
      </Show>
    </div>
  );
};

export default LibraryViewRoute;
