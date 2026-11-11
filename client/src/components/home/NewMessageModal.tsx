/**
 * NewMessageModal Component
 *
 * Modal for creating a new DM conversation. Offers friends and everyone who
 * shares a server with me (NAV-1, decision D1: no friendship needed), friends
 * first.
 */

import { Component, createSignal, For, Show, createMemo, onMount } from "solid-js";
import { Portal } from "solid-js/web";
import { X, Search, Check } from "lucide-solid";
import { t, plural } from "@/i18n";
import { errorText } from "@/i18n/errors";
import { friendsState, loadFriends, loadBlocked } from "@/stores/friends";
import { openDirectMessage } from "@/stores/dms";
import { guildsState, loadAllGuildMembers } from "@/stores/guilds";
import { currentUser } from "@/stores/auth";
import { createDialog } from "@/lib/a11y/dialog";

interface NewMessageModalProps {
  onClose: () => void;
}

/** A person I can write to. */
export interface MessageCandidate {
  user_id: string;
  username: string;
  display_name: string;
  /** Account name when a server nickname is shown instead (K6). */
  real_name?: string;
  isFriend: boolean;
}

const byName = (a: MessageCandidate, b: MessageCandidate) =>
  a.display_name.localeCompare(b.display_name);

/**
 * Friends first, then members of my servers; never me, never twice, never
 * someone I blocked (W4). The client only knows my own blocks; blocks the
 * other way are refused by the server.
 */
export function messageCandidates(): MessageCandidate[] {
  const me = currentUser()?.id;
  const seen = new Set<string>(me ? [me] : []);
  for (const b of friendsState.blocked) seen.add(b.user_id);
  const friends: MessageCandidate[] = [];
  for (const f of friendsState.friends) {
    if (seen.has(f.user_id)) continue;
    seen.add(f.user_id);
    friends.push({ user_id: f.user_id, username: f.username, display_name: f.display_name, isFriend: true });
  }
  const others: MessageCandidate[] = [];
  for (const g of guildsState.guilds) {
    for (const m of guildsState.members[g.id] ?? []) {
      if (seen.has(m.user_id)) continue;
      seen.add(m.user_id);
      others.push({
        user_id: m.user_id,
        username: m.username,
        display_name: m.nickname || m.display_name,
        real_name: m.nickname ? m.display_name : undefined,
        isFriend: false,
      });
    }
  }
  return [...friends.sort(byName), ...others.sort(byName)];
}

const NewMessageModal: Component<NewMessageModalProps> = (props) => {
  let searchInput: HTMLInputElement | undefined;
  const dialog = createDialog({
    onClose: () => props.onClose(),
    initialFocus: () => searchInput,
  });

  const [search, setSearch] = createSignal("");
  const [selectedIds, setSelectedIds] = createSignal<string[]>([]);
  const [isCreating, setIsCreating] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  // Member lists load fresh on every open (people join and leave); show
  // that it is loading and when a server could not be read (K4, K5).
  const [isLoadingPeople, setIsLoadingPeople] = createSignal(true);
  const [someFailed, setSomeFailed] = createSignal(false);

  onMount(() => {
    if (friendsState.friends.length === 0) void loadFriends();
    // Blocks must be known before anyone is offered (W4).
    void Promise.all([loadAllGuildMembers(), loadBlocked()])
      .then(([failed]) => setSomeFailed(failed > 0))
      .finally(() => setIsLoadingPeople(false));
  });

  const filteredPeople = createMemo(() => {
    const searchLower = search().trim().toLowerCase();
    const all = messageCandidates();
    if (!searchLower) return all;
    return all.filter(
      (p) =>
        p.username.toLowerCase().includes(searchLower) ||
        p.display_name.toLowerCase().includes(searchLower) ||
        (p.real_name?.toLowerCase().includes(searchLower) ?? false),
    );
  });

  const emptyListText = () => {
    if (isLoadingPeople()) return t("dm.new.loadingPeople");
    return search() ? t("dm.new.noPeopleFound") : t("dm.new.noPeople");
  };

  const submitLabel = () => {
    if (isCreating()) return t("dm.new.creating");
    if (selectedIds().length > 1) return t("dm.new.createGroup");
    return t("dm.new.create");
  };

  const togglePerson = (userId: string) => {
    setSelectedIds((prev) =>
      prev.includes(userId)
        ? prev.filter((id) => id !== userId)
        : [...prev, userId],
    );
  };

  const handleCreate = async () => {
    if (selectedIds().length === 0) return;

    setIsCreating(true);
    setError(null);

    try {
      await openDirectMessage(selectedIds());
      props.onClose();
    } catch (err) {
      setError(errorText(err, "dm.new.createFailed"));
    } finally {
      setIsCreating(false);
    }
  };

  return (
    <Portal>
      <div
        class="fixed inset-0 bg-black/50 flex items-center justify-center z-50"
        onClick={props.onClose}
      >
        <div
          ref={dialog.ref}
          {...dialog.dialogProps}
          class="border border-white/10 rounded-2xl w-full max-w-md mx-4 flex flex-col max-h-[80vh] outline-none"
          style={{"background-color":"var(--color-surface-base)"}}
          onClick={(e) => e.stopPropagation()}
        >
          {/* Header */}
          <div class="flex items-center justify-between p-4 border-b border-white/10">
            <h2 id={dialog.titleId} class="text-lg font-bold text-text-primary">{t("dm.new.title")}</h2>
            <button
              onClick={props.onClose}
              class="p-1 rounded hover:bg-white/10 transition-colors"
              aria-label={t("common.close")}
            >
              <X class="w-5 h-5 text-text-secondary" aria-hidden="true" />
            </button>
          </div>

          {/* Search */}
          <div class="p-4 border-b border-white/10">
            <div class="relative">
              <Search class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-text-secondary" />
              <input
                ref={searchInput}
                data-testid="new-dm-input"
                aria-label={t("dm.new.searchPlaceholder")}
                type="text"
                value={search()}
                onInput={(e) => setSearch(e.currentTarget.value)}
                placeholder={t("dm.new.searchPlaceholder")}
                class="w-full pl-9 pr-4 py-2 border border-white/10 rounded-lg text-text-input placeholder-text-placeholder focus:outline-none focus:border-accent-primary"
                style={{"background-color":"var(--color-surface-layer1)"}}
              />
            </div>
            <Show when={selectedIds().length > 0}>
              <p class="mt-2 text-sm text-text-secondary">
                {plural(selectedIds().length, {
                  one: "dm.new.selected.one",
                  other: "dm.new.selected.other",
                })}
                <Show when={selectedIds().length > 1}>
                  <span class="ml-1">{t("dm.new.groupHint")}</span>
                </Show>
              </p>
            </Show>
          </div>

          <Show when={someFailed()}>
            <p data-testid="new-dm-partial" role="status" class="mx-4 mt-2 text-sm text-text-secondary">
              {t("dm.new.someNotLoaded")}
            </p>
          </Show>

          {/* People */}
          <div class="flex-1 overflow-y-auto p-2">
            <Show
              when={filteredPeople().length > 0}
              fallback={
                <div data-testid="new-dm-empty" role="status" class="text-center py-8 text-text-secondary">
                  {emptyListText()}
                </div>
              }
            >
              <ul class="space-y-0.5" aria-label={t("dm.new.peopleLabel")}>
                <For each={filteredPeople()}>
                  {(person) => (
                    <li>
                      <PersonSelectItem
                        person={person}
                        selected={selectedIds().includes(person.user_id)}
                        onToggle={() => togglePerson(person.user_id)}
                      />
                    </li>
                  )}
                </For>
              </ul>
            </Show>
          </div>

          {/* Error */}
          <Show when={error()}>
            <div
              class="mx-4 mb-2 p-3 rounded-lg text-sm"
              style={{"background-color":"var(--color-error-bg)","border":"1px solid var(--color-error-border)","color":"var(--color-error-text)"}}
            >
              {error()}
            </div>
          </Show>

          {/* Footer */}
          <div class="p-4 border-t border-white/10">
            <button
              data-testid="new-dm-submit"
              onClick={handleCreate}
              disabled={selectedIds().length === 0 || isCreating()}
              class="w-full py-2 bg-accent-primary text-on-accent rounded-lg font-medium hover:opacity-90 transition-opacity state-disabled"
            >
              {submitLabel()}
            </button>
          </div>
        </div>
      </div>
    </Portal>
  );
};

interface PersonSelectItemProps {
  person: MessageCandidate;
  selected: boolean;
  onToggle: () => void;
}

const PersonSelectItem: Component<PersonSelectItemProps> = (props) => {
  return (
    <button
      type="button"
      data-testid="new-dm-person"
      role="checkbox"
      aria-checked={props.selected}
      onClick={props.onToggle}
      class="w-full flex items-center gap-3 p-2 rounded-lg hover:bg-white/5 transition-colors"
    >
      {/* Checkbox */}
      <div
        aria-hidden="true"
        class="w-5 h-5 rounded border-2 flex items-center justify-center transition-colors"
        classList={{
          "border-accent-primary bg-accent-primary": props.selected,
          "border-white/30": !props.selected,
        }}
      >
        <Show when={props.selected}>
          <Check class="w-3 h-3 text-on-accent" />
        </Show>
      </div>

      {/* Avatar */}
      <div aria-hidden="true" class="w-8 h-8 rounded-full bg-accent-primary flex items-center justify-center">
        <span class="text-xs font-semibold text-on-accent">
          {props.person.display_name.charAt(0).toUpperCase()}
        </span>
      </div>

      {/* Name */}
      <div class="flex-1 text-left min-w-0">
        <div class="font-medium text-text-primary truncate">
          {props.person.display_name}
        </div>
        <div class="text-sm text-text-secondary truncate">
          @{props.person.username}
          <Show when={props.person.real_name}>
            <span> · {props.person.real_name}</span>
          </Show>
        </div>
      </div>

      <Show when={props.person.isFriend}>
        <span class="text-xs text-text-secondary flex-shrink-0">{t("dm.new.friendTag")}</span>
      </Show>
    </button>
  );
};

export default NewMessageModal;
