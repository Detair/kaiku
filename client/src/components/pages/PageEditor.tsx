/**
 * Page Editor Component
 *
 * Side-by-side markdown editor with live preview.
 */

import { createSignal, createEffect, Show, For, onCleanup } from "solid-js";
import {
  Bold,
  Italic,
  Strikethrough,
  Image,
  Code,
  List,
  ListOrdered,
  Link,
  Eye,
  EyeOff,
  Save,
  X,
} from "lucide-solid";
import type { Page, PageCategory } from "@/lib/types";
import { MAX_CONTENT_SIZE } from "@/lib/pageConstants";
import MarkdownPreview from "./MarkdownPreview";
import MarkdownCheatSheet from "./MarkdownCheatSheet";
import { createConfirm } from "@/components/admin/confirmHost";
import { actionErrorText } from "@/components/admin/actionErrors";
import { t } from "@/i18n";

interface PageEditorProps {
  page?: Page | null;
  guildId?: string;
  categories?: PageCategory[];
  onSave: (data: {
    title: string;
    slug: string;
    content: string;
    requiresAcceptance: boolean;
    categoryId?: string | null;
  }) => Promise<void>;
  onCancel: () => void;
  isPlatform?: boolean;
}

/**
 * Generate slug from title.
 */
function slugify(title: string): string {
  return title
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 100);
}

export default function PageEditor(props: PageEditorProps) {
  const [title, setTitle] = createSignal(props.page?.title || "");
  const [slug, setSlug] = createSignal(props.page?.slug || "");
  const [content, setContent] = createSignal(props.page?.content || "");
  const [categoryId, setCategoryId] = createSignal<string | null>(
    props.page?.category_id ?? null,
  );
  const [requiresAcceptance, setRequiresAcceptance] = createSignal(
    props.page?.requires_acceptance || false,
  );
  const [showPreview, setShowPreview] = createSignal(true);
  const [isSaving, setIsSaving] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [hasUnsavedChanges, setHasUnsavedChanges] = createSignal(false);
  const [slugManuallyEdited, setSlugManuallyEdited] = createSignal(
    !!props.page?.slug,
  );

  let textareaRef: HTMLTextAreaElement | undefined;

  // Sync signals when props.page changes (e.g. navigating to a different page)
  createEffect(() => {
    const page = props.page;
    setTitle(page?.title || "");
    setSlug(page?.slug || "");
    setContent(page?.content || "");
    setCategoryId(page?.category_id ?? null);
    setRequiresAcceptance(page?.requires_acceptance || false);
    setSlugManuallyEdited(!!page?.slug);
    setHasUnsavedChanges(false);
    setError(null);
  });

  // Track unsaved changes
  createEffect(() => {
    const hasChanges =
      title() !== (props.page?.title || "") ||
      slug() !== (props.page?.slug || "") ||
      content() !== (props.page?.content || "") ||
      categoryId() !== (props.page?.category_id ?? null) ||
      requiresAcceptance() !== (props.page?.requires_acceptance || false);

    setHasUnsavedChanges(hasChanges);
  });

  // Auto-generate slug from title (unless manually edited)
  createEffect(() => {
    if (!slugManuallyEdited() && title()) {
      setSlug(slugify(title()));
    }
  });

  // Warn before leaving with unsaved changes
  createEffect(() => {
    const handleBeforeUnload = (e: BeforeUnloadEvent) => {
      if (hasUnsavedChanges()) {
        e.preventDefault();
        e.returnValue = "";
      }
    };

    window.addEventListener("beforeunload", handleBeforeUnload);
    onCleanup(() =>
      window.removeEventListener("beforeunload", handleBeforeUnload),
    );
  });

  const contentSize = () => new TextEncoder().encode(content()).length;
  const isContentTooLarge = () => contentSize() > MAX_CONTENT_SIZE;

  // Insert text at cursor position
  const insertText = (before: string, after: string = "") => {
    if (!textareaRef) return;

    const start = textareaRef.selectionStart;
    const end = textareaRef.selectionEnd;
    const selected = content().slice(start, end);

    const newContent =
      content().slice(0, start) +
      before +
      selected +
      after +
      content().slice(end);

    setContent(newContent);

    // Restore cursor position
    requestAnimationFrame(() => {
      if (textareaRef) {
        const cursorPos = start + before.length + selected.length;
        textareaRef.focus();
        textareaRef.setSelectionRange(cursorPos, cursorPos);
      }
    });
  };

  const handleSave = async () => {
    if (!title().trim()) {
      setError("Title is required");
      return;
    }

    if (!slug().trim()) {
      setError("Slug is required");
      return;
    }

    if (!content().trim()) {
      setError("Content is required");
      return;
    }

    if (isContentTooLarge()) {
      setError("Content exceeds maximum size (100KB)");
      return;
    }

    // Validate slug format
    if (!/^[a-z0-9]([a-z0-9-]*[a-z0-9])?$/.test(slug())) {
      setError(
        "Invalid slug format. Use lowercase letters, numbers, and single dashes (e.g., 'terms-of-service')",
      );
      return;
    }

    setIsSaving(true);
    setError(null);

    try {
      await props.onSave({
        title: title().trim(),
        slug: slug().trim(),
        content: content(),
        requiresAcceptance: requiresAcceptance(),
        categoryId: props.categories ? categoryId() : undefined,
      });
      setHasUnsavedChanges(false);
    } catch (err) {
      setError(`${t("guildSettings.pages.saveFailed")} ${actionErrorText(err)}`);
    } finally {
      setIsSaving(false);
    }
  };

  const confirmDialog = createConfirm();
  const handleCancel = () => {
    if (hasUnsavedChanges()) {
      confirmDialog.ask({
        title: t("guildSettings.unsaved.title"),
        message: t("guildSettings.unsaved.body"),
        confirmLabel: t("guildSettings.unsaved.discard"),
        danger: true,
        run: () => props.onCancel(),
      });
      return;
    }
    props.onCancel();
  };

  const toolbarButtons = [
    { icon: Bold, title: "Bold", action: () => insertText("**", "**") },
    { icon: Italic, title: "Italic", action: () => insertText("*", "*") },
    {
      icon: Strikethrough,
      title: "Strikethrough",
      action: () => insertText("~~", "~~"),
    },
    { icon: Code, title: "Code", action: () => insertText("`", "`") },
    { icon: Link, title: "Link", action: () => insertText("[", "](url)") },
    { icon: Image, title: "Image", action: () => insertText("![alt](", ")") },
    { icon: List, title: "Bullet List", action: () => insertText("- ") },
    {
      icon: ListOrdered,
      title: "Numbered List",
      action: () => insertText("1. "),
    },
  ];

  return (
    <div class="flex flex-col h-full bg-surface-base">
      {/* Header */}
      <div class="flex items-center justify-between px-4 py-3 border-b border-white/10">
        <h2 class="text-lg font-semibold text-text-primary">
          {props.page ? "Edit Page" : "New Page"}
        </h2>
        <div class="flex items-center gap-2">
          <Show when={hasUnsavedChanges()}>
            <span class="text-xs text-text-primary">Unsaved changes</span>
          </Show>
          <button
            type="button"
            onClick={handleCancel}
            class="px-3 py-1.5 text-sm text-text-secondary hover:text-text-primary transition-colors"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={handleSave}
            data-testid="page-editor-save"
            disabled={isSaving() || isContentTooLarge()}
            class="px-4 py-1.5 text-sm font-medium text-on-accent bg-accent-primary hover:bg-accent-primary/90 rounded-md flex items-center gap-2 transition-colors state-disabled"
          >
            <Save class="w-4 h-4" />
            {isSaving() ? "Saving..." : "Save"}
          </button>
        </div>
      </div>

      {/* Error message */}
      <Show when={error()}>
        <div class="mx-4 mt-3 px-3 py-2 bg-status-error/15 border border-status-error/50 rounded-md text-sm text-text-primary flex items-center gap-2">
          <X class="w-4 h-4 flex-shrink-0" />
          {error()}
        </div>
      </Show>

      {/* Title and slug inputs */}
      <div class="px-4 py-3 space-y-3 border-b border-white/10">
        <div>
          <label class="block text-sm font-medium text-text-primary mb-1">
            Title
          </label>
          <input
            type="text"
            data-testid="page-editor-title"
            value={title()}
            onInput={(e) => setTitle(e.currentTarget.value)}
            placeholder="Page title"
            maxLength={100}
            class="w-full px-3 py-2 bg-surface-layer1 border border-white/15 rounded-md text-text-primary placeholder-text-placeholder focus:outline-none focus:ring-2 focus:ring-accent-primary focus:border-transparent"
          />
        </div>
        <div>
          <label class="block text-sm font-medium text-text-primary mb-1">
            Slug
            <span class="text-text-secondary font-normal ml-2">
              (URL: /
              {props.isPlatform ? "pages" : `guild/${props.guildId}/pages`}/
              {slug() || "..."})
            </span>
          </label>
          <input
            type="text"
            data-testid="page-editor-slug"
            value={slug()}
            onInput={(e) => {
              setSlug(
                e.currentTarget.value.toLowerCase().replace(/[^a-z0-9-]/g, ""),
              );
              setSlugManuallyEdited(true);
            }}
            placeholder="page-slug"
            maxLength={100}
            class="w-full px-3 py-2 bg-surface-layer1 border border-white/15 rounded-md text-text-primary placeholder-text-placeholder focus:outline-none focus:ring-2 focus:ring-accent-primary focus:border-transparent font-mono text-sm"
          />
        </div>
        <Show when={props.categories && props.categories.length > 0}>
          <div>
            <label class="block text-sm font-medium text-text-primary mb-1">
              Category
            </label>
            <select
              value={categoryId() ?? ""}
              onChange={(e) => setCategoryId(e.currentTarget.value || null)}
              class="w-full px-3 py-2 bg-surface-layer1 border border-white/15 rounded-md text-text-primary focus:outline-none focus:ring-2 focus:ring-accent-primary focus:border-transparent"
            >
              <option value="">Uncategorized</option>
              <For each={props.categories}>
                {(cat) => <option value={cat.id}>{cat.name}</option>}
              </For>
            </select>
          </div>
        </Show>
        <div class="flex items-center gap-3">
          <label class="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              data-testid="page-editor-requires-acceptance"
              checked={requiresAcceptance()}
              onChange={(e) => setRequiresAcceptance(e.currentTarget.checked)}
              class="w-4 h-4 rounded border-white/15 bg-surface-layer1 text-accent-primary focus:ring-accent-primary focus:ring-offset-surface-base"
            />
            <span class="text-sm text-text-primary">Require user acceptance</span>
          </label>
          <Show when={requiresAcceptance()}>
            <span class="text-xs text-text-secondary">
              {props.isPlatform
                ? "Users must accept before using the platform"
                : "Users will be prompted to accept this page"}
            </span>
          </Show>
        </div>
      </div>

      {/* Toolbar */}
      <div class="px-4 py-2 flex items-center gap-1 border-b border-white/10">
        <For each={toolbarButtons}>{(btn) => (
          <button
            type="button"
            onClick={btn.action}
            title={btn.title}
            class="p-2 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded transition-colors"
          >
            <btn.icon class="w-4 h-4" />
          </button>
        )}</For>
        <div class="flex-1" />
        <div class="text-xs text-text-secondary mr-2">
          {contentSize().toLocaleString()} / {MAX_CONTENT_SIZE.toLocaleString()}{" "}
          bytes
          <Show when={isContentTooLarge()}>
            <span class="text-text-primary ml-1">(too large)</span>
          </Show>
        </div>
        <button
          type="button"
          onClick={() => setShowPreview(!showPreview())}
          title={showPreview() ? "Hide preview" : "Show preview"}
          class={`p-2 rounded transition-colors ${
            showPreview()
              ? "text-text-primary bg-accent-primary/20"
              : "text-text-secondary hover:text-text-primary hover:bg-white/10"
          }`}
        >
          <Show when={showPreview()} fallback={<EyeOff class="w-4 h-4" />}>
            <Eye class="w-4 h-4" />
          </Show>
        </button>
      </div>

      {/* Editor and Preview */}
      <div class="flex-1 flex overflow-hidden">
        {/* Editor */}
        <div
          class={`flex flex-col ${showPreview() ? "w-1/2" : "w-full"} border-r border-white/10`}
        >
          <textarea
            ref={textareaRef}
            data-testid="page-editor-content"
            value={content()}
            onInput={(e) => setContent(e.currentTarget.value)}
            placeholder="Write your content in Markdown..."
            class="flex-1 w-full p-4 bg-surface-base text-text-primary placeholder-text-placeholder resize-none focus:outline-none font-mono text-sm leading-relaxed"
            spellcheck={false}
          />
          <div class="p-2 border-t border-white/10">
            <MarkdownCheatSheet />
          </div>
        </div>

        {/* Preview */}
        <Show when={showPreview()}>
          <div class="w-1/2 overflow-auto p-4 bg-surface-layer1">
            <Show
              when={content()}
              fallback={
                <p class="text-text-secondary italic">Preview will appear here...</p>
              }
            >
              <MarkdownPreview content={content()} />
            </Show>
          </div>
        </Show>
      </div>
      <confirmDialog.Host />
    </div>
  );
}
