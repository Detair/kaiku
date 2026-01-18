/**
 * Acceptance Manager Component
 *
 * Manages the page acceptance flow for both platform and guild pages.
 * Platform pages are blocking (must accept to continue).
 * Guild pages are non-blocking (can defer).
 */

import { Show, createSignal, onMount } from "solid-js";
import type { Page, PageListItem } from "@/lib/types";
import {
  pagesState,
  loadPendingAcceptance,
  acceptPage as acceptPageAction,
} from "@/stores/pages";
import * as tauri from "@/lib/tauri";
import PageAcceptanceModal from "./PageAcceptanceModal";

interface AcceptanceManagerProps {
  /** Called when user logs out (for blocking platform pages) */
  onLogout?: () => void;
}

export default function AcceptanceManager(_props: AcceptanceManagerProps) {
  const [currentPage, setCurrentPage] = createSignal<Page | null>(null);
  const [isLoading, setIsLoading] = createSignal(false);
  const [deferredPageIds, setDeferredPageIds] = createSignal<Set<string>>(new Set());

  // Load pending pages on mount
  onMount(async () => {
    await loadPendingAcceptance();
    await showNextPage();
  });

  // Get the next page to show (platform pages first, then guild pages)
  const getNextPendingPage = (): PageListItem | null => {
    const pending = pagesState.pendingAcceptance;
    const deferred = deferredPageIds();

    // First, check for platform pages (blocking)
    const platformPage = pending.find((p) => p.guild_id === null && !deferred.has(p.id));
    if (platformPage) return platformPage;

    // Then, check for guild pages (non-blocking, can be deferred)
    const guildPage = pending.find((p) => p.guild_id !== null && !deferred.has(p.id));
    return guildPage || null;
  };

  // Show the next page that needs acceptance
  const showNextPage = async () => {
    const nextPageListItem = getNextPendingPage();

    if (!nextPageListItem) {
      setCurrentPage(null);
      return;
    }

    setIsLoading(true);
    try {
      // Load the full page content
      const fullPage = nextPageListItem.guild_id
        ? await tauri.getGuildPage(nextPageListItem.guild_id, nextPageListItem.slug)
        : await tauri.getPlatformPage(nextPageListItem.slug);

      setCurrentPage(fullPage);
    } catch (err) {
      console.error("Failed to load page for acceptance:", err);
      // If we can't load the page, skip it
      setDeferredPageIds((prev) => new Set([...prev, nextPageListItem.id]));
      await showNextPage();
    } finally {
      setIsLoading(false);
    }
  };

  // Handle page acceptance
  const handleAccept = async () => {
    const page = currentPage();
    if (!page) return;

    await acceptPageAction(page.id);
    setCurrentPage(null);

    // Show next page if any
    await showNextPage();
  };

  // Handle deferring (guild pages only)
  const handleDefer = () => {
    const page = currentPage();
    if (!page || page.guild_id === null) return;

    setDeferredPageIds((prev) => new Set([...prev, page.id]));
    setCurrentPage(null);

    // Show next page if any
    showNextPage();
  };

  // Handle close (same as defer for non-blocking)
  const handleClose = () => {
    const page = currentPage();
    if (!page) return;

    if (page.guild_id === null) {
      // Platform pages are blocking - can't close
      return;
    }

    handleDefer();
  };

  // Check if current page is blocking (platform page)
  const isBlocking = () => {
    const page = currentPage();
    return page !== null && page.guild_id === null;
  };

  return (
    <Show when={currentPage() && !isLoading()}>
      <PageAcceptanceModal
        page={currentPage()!}
        isBlocking={isBlocking()}
        onAccept={handleAccept}
        onDefer={isBlocking() ? undefined : handleDefer}
        onClose={isBlocking() ? undefined : handleClose}
      />
    </Show>
  );
}
