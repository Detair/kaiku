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
      setError(err instanceof Error ? err.message : "Failed to save page");
    } finally {
      setIsSaving(false);
    }
  };

  const handleCancel = () => {
    if (hasUnsavedChanges()) {
      if (
        !confirm("You have unsaved changes. Are you sure you want to leave?")
      ) {
        return;
      }
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
    <div class="flex flex-col h-full bg-zinc-900">
      {/* Header */}
      <div class="flex items-center justify-between px-4 py-3 border-b border-zinc-700">
        <h2 class="text-lg font-semibold text-white">
          {props.page ? "Edit Page" : "New Page"}
        </h2>
        <div class="flex items-center gap-2">
          <Show when={hasUnsavedChanges()}>
            <span class="text-xs text-amber-400">Unsaved changes</span>
          </Show>
          <button
            type="button"
            onClick={handleCancel}
            class="px-3 py-1.5 text-sm text-zinc-400 hover:text-white transition-colors"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={handleSave}
            disabled={isSaving() || isContentTooLarge()}
            class="px-4 py-1.5 text-sm font-medium text-white bg-emerald-600 hover:bg-emerald-500 disabled:opacity-50 disabled:cursor-not-allowed rounded-md flex items-center gap-2 transition-colors"
          >
            <Save class="w-4 h-4" />
            {isSaving() ? "Saving..." : "Save"}
          </button>
        </div>
      </div>

      {/* Error message */}
      <Show when={error()}>
        <div class="mx-4 mt-3 px-3 py-2 bg-red-900/30 border border-red-700 rounded-md text-sm text-red-300 flex items-center gap-2">
          <X class="w-4 h-4 flex-shrink-0" />
          {error()}
        </div>
      </Show>

      {/* Title and slug inputs */}
      <div class="px-4 py-3 space-y-3 border-b border-zinc-700">
        <div>
          <label class="block text-sm font-medium text-zinc-300 mb-1">
            Title
          </label>
          <input
            type="text"
            value={title()}
            onInput={(e) => setTitle(e.currentTarget.value)}
            placeholder="Page title"
            maxLength={100}
            class="w-full px-3 py-2 bg-zinc-800 border border-zinc-600 rounded-md text-white placeholder-zinc-500 focus:outline-none focus:ring-2 focus:ring-emerald-500 focus:border-transparent"
          />
        </div>
        <div>
          <label class="block text-sm font-medium text-zinc-300 mb-1">
            Slug
            <span class="text-zinc-500 font-normal ml-2">
              (URL: /
              {props.isPlatform ? "pages" : `guild/${props.guildId}/pages`}/
              {slug() || "..."})
            </span>
          </label>
          <input
            type="text"
            value={slug()}
            onInput={(e) => {
              setSlug(
                e.currentTarget.value.toLowerCase().replace(/[^a-z0-9-]/g, ""),
              );
              setSlugManuallyEdited(true);
            }}
            placeholder="page-slug"
            maxLength={100}
            class="w-full px-3 py-2 bg-zinc-800 border border-zinc-600 rounded-md text-white placeholder-zinc-500 focus:outline-none focus:ring-2 focus:ring-emerald-500 focus:border-transparent font-mono text-sm"
          />
        </div>
        <Show when={props.categories && props.categories.length > 0}>
          <div>
            <label class="block text-sm font-medium text-zinc-300 mb-1">
              Category
            </label>
            <select
              value={categoryId() ?? ""}
              onChange={(e) => setCategoryId(e.currentTarget.value || null)}
              class="w-full px-3 py-2 bg-zinc-800 border border-zinc-600 rounded-md text-white focus:outline-none focus:ring-2 focus:ring-emerald-500 focus:border-transparent"
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
              checked={requiresAcceptance()}
              onChange={(e) => setRequiresAcceptance(e.currentTarget.checked)}
              class="w-4 h-4 rounded border-zinc-600 bg-zinc-800 text-emerald-500 focus:ring-emerald-500 focus:ring-offset-zinc-900"
            />
            <span class="text-sm text-zinc-300">Require user acceptance</span>
          </label>
          <Show when={requiresAcceptance()}>
            <span class="text-xs text-zinc-500">
              {props.isPlatform
                ? "Users must accept before using the platform"
                : "Users will be prompted to accept this page"}
            </span>
          </Show>
        </div>
      </div>

      {/* Toolbar */}
      <div class="px-4 py-2 flex items-center gap-1 border-b border-zinc-700">
        {toolbarButtons.map((btn) => (
          <button
            type="button"
            onClick={btn.action}
            title={btn.title}
            class="p-2 text-zinc-400 hover:text-white hover:bg-zinc-700 rounded transition-colors"
          >
            <btn.icon class="w-4 h-4" />
          </button>
        ))}
        <div class="flex-1" />
        <div class="text-xs text-zinc-500 mr-2">
          {contentSize().toLocaleString()} / {MAX_CONTENT_SIZE.toLocaleString()}{" "}
          bytes
          <Show when={isContentTooLarge()}>
            <span class="text-red-400 ml-1">(too large)</span>
          </Show>
        </div>
        <button
          type="button"
          onClick={() => setShowPreview(!showPreview())}
          title={showPreview() ? "Hide preview" : "Show preview"}
          class={`p-2 rounded transition-colors ${
            showPreview()
              ? "text-emerald-400 bg-emerald-900/30"
              : "text-zinc-400 hover:text-white hover:bg-zinc-700"
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
          class={`flex flex-col ${showPreview() ? "w-1/2" : "w-full"} border-r border-zinc-700`}
        >
          <textarea
            ref={textareaRef}
            value={content()}
            onInput={(e) => setContent(e.currentTarget.value)}
            placeholder="Write your content in Markdown..."
            class="flex-1 w-full p-4 bg-zinc-900 text-white placeholder-zinc-500 resize-none focus:outline-none font-mono text-sm leading-relaxed"
            spellcheck={false}
          />
          <div class="p-2 border-t border-zinc-700">
            <MarkdownCheatSheet />
          </div>
        </div>

        {/* Preview */}
        <Show when={showPreview()}>
          <div class="w-1/2 overflow-auto p-4 bg-zinc-800">
            <Show
              when={content()}
              fallback={
                <p class="text-zinc-500 italic">Preview will appear here...</p>
              }
            >
              <MarkdownPreview content={content()} />
            </Show>
          </div>
        </Show>
      </div>
    </div>
  );
}
