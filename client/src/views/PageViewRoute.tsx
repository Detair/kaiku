/**
 * Page View Route
 *
 * Route component for viewing a single page (platform or guild).
 * URL patterns:
 * - /pages/:slug - Platform page
 * - /guilds/:guildId/pages/:slug - Guild page
 */

import { Component, Show, createResource, createEffect } from "solid-js";
import { useParams, useNavigate } from "@solidjs/router";
import { ArrowLeft } from "lucide-solid";
import { PageView } from "@/components/pages";
import * as tauri from "@/lib/tauri";

const PageViewRoute: Component = () => {
  const params = useParams<{ slug: string; guildId?: string }>();
  const navigate = useNavigate();

  const [page] = createResource(
    () => ({ slug: params.slug, guildId: params.guildId }),
    async ({ slug, guildId }) => {
      if (!slug) return null;

      if (guildId) {
        return tauri.getGuildPage(guildId, slug);
      }
      return tauri.getPlatformPage(slug);
    },
  );

  // Scroll to URL fragment after page loads and markdown renders
  createEffect(() => {
    if (page() && window.location.hash) {
      const hash = window.location.hash.slice(1);
      // Delay to allow markdown rendering to complete
      setTimeout(() => {
        const el = document.getElementById(hash);
        el?.scrollIntoView({ behavior: "smooth" });
      }, 200);
    }
  });

  const handleBack = () => {
    navigate(-1);
  };

  return (
    <div class="h-screen bg-zinc-900 flex flex-col">
      <Show
        when={!page.loading}
        fallback={
          <div class="flex-1 flex items-center justify-center">
            <div class="text-zinc-400">Loading page...</div>
          </div>
        }
      >
        <Show
          when={page()}
          fallback={
            <div class="flex-1 flex flex-col items-center justify-center">
              <div class="text-zinc-400 mb-4">Page not found</div>
              <button
                type="button"
                onClick={handleBack}
                class="flex items-center gap-2 px-4 py-2 text-sm font-medium text-zinc-300 hover:text-white hover:bg-zinc-700 rounded-md transition-colors"
              >
                <ArrowLeft class="w-4 h-4" />
                Go back
              </button>
            </div>
          }
        >
          <PageView page={page()!} onBack={handleBack} />
        </Show>
      </Show>
    </div>
  );
};

export default PageViewRoute;
