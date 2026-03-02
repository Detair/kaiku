import { Component, For, Show, createSignal, onMount } from "solid-js";
import { BookOpen, Plus, RefreshCw } from "lucide-solid";
import PageEditor from "@/components/pages/PageEditor";
import {
  createPlatformPage,
  loadPlatformPages,
  pagesState,
} from "@/stores/pages";

const PlatformPagesPanel: Component = () => {
  const [isCreating, setIsCreating] = createSignal(false);
  const [isRefreshing, setIsRefreshing] = createSignal(false);

  onMount(() => {
    void loadPlatformPages();
  });

  const handleRefresh = async () => {
    setIsRefreshing(true);
    try {
      await loadPlatformPages();
    } finally {
      setIsRefreshing(false);
    }
  };

  const handleSave = async (data: {
    title: string;
    slug: string;
    content: string;
    requiresAcceptance: boolean;
    categoryId?: string | null;
  }) => {
    const created = await createPlatformPage(
      data.title,
      data.content,
      data.slug,
      data.requiresAcceptance,
    );

    if (!created) {
      throw new Error(pagesState.error ?? "Failed to create platform page");
    }

    setIsCreating(false);
  };

  return (
    <div class="flex-1 flex flex-col overflow-hidden" data-testid="platform-pages-panel">
      <Show when={isCreating()} fallback={
        <div class="flex-1 p-6 overflow-auto">
          <div class="max-w-4xl mx-auto space-y-6">
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-3">
                <BookOpen class="w-5 h-5 text-accent-primary" />
                <h2 class="text-lg font-bold text-text-primary">Platform Pages</h2>
              </div>
              <div class="flex items-center gap-2">
                <button
                  type="button"
                  onClick={handleRefresh}
                  disabled={isRefreshing() || pagesState.isPlatformLoading}
                  class="px-3 py-1.5 rounded-lg text-sm text-text-secondary hover:text-text-primary hover:bg-white/5 disabled:opacity-50 transition-colors"
                >
                  <span class="inline-flex items-center gap-2">
                    <RefreshCw class="w-4 h-4" classList={{ "animate-spin": isRefreshing() }} />
                    Refresh
                  </span>
                </button>
                <button
                  type="button"
                  data-testid="new-platform-page"
                  onClick={() => setIsCreating(true)}
                  class="px-3 py-1.5 rounded-lg text-sm font-medium bg-accent-primary text-white hover:bg-accent-primary/90 transition-colors inline-flex items-center gap-2"
                >
                  <Plus class="w-4 h-4" />
                  New Platform Page
                </button>
              </div>
            </div>

            <Show when={!pagesState.isPlatformLoading} fallback={<div class="text-text-secondary">Loading pages...</div>}>
              <Show when={pagesState.platformPages.length > 0} fallback={<div class="text-text-secondary">No platform pages yet.</div>}>
                <div class="space-y-2">
                  <For each={pagesState.platformPages}>
                    {(page) => (
                      <div class="p-3 rounded-lg bg-white/5 border border-white/10 flex items-center justify-between">
                        <div>
                          <div class="text-sm font-medium text-text-primary">{page.title}</div>
                          <div class="text-xs text-text-secondary">/{page.slug}</div>
                        </div>
                        <Show when={page.requires_acceptance}>
                          <span class="px-2 py-0.5 rounded text-xs bg-amber-500/20 text-amber-400">
                            Requires Acceptance
                          </span>
                        </Show>
                      </div>
                    )}
                  </For>
                </div>
              </Show>
            </Show>
          </div>
        </div>
      }>
        <PageEditor isPlatform onSave={handleSave} onCancel={() => setIsCreating(false)} />
      </Show>
    </div>
  );
};

export default PlatformPagesPanel;
