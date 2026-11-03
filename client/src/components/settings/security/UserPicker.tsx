/**
 * UserPicker — choose another person of this server. There is no general
 * user search on the server, so the list is the user's friends (a recovery
 * contact is someone the user knows), filtered by name.
 */

import { Component, For, Show, createResource, createSignal } from "solid-js";
import { t } from "@/i18n";
import { getFriends } from "@/lib/tauri/social";

export interface PickedUser {
  user_id: string;
  username: string;
  display_name: string;
}

export interface UserPickerProps {
  onPick: (u: PickedUser) => void;
  onCancel: () => void;
  /** Users who cannot be chosen (yourself, the current contact). */
  exclude?: string[];
}

const UserPicker: Component<UserPickerProps> = (props) => {
  const [filter, setFilter] = createSignal("");
  const [friends] = createResource(async () => {
    try {
      return await getFriends();
    } catch {
      return [];
    }
  });
  const shown = () => {
    const q = filter().trim().toLowerCase();
    return (friends() ?? [])
      .filter((f) => !(props.exclude ?? []).includes(f.user_id))
      .filter((f) => !q || f.username.toLowerCase().includes(q) || f.display_name.toLowerCase().includes(q));
  };
  return (
    <div class="space-y-3">
      <label class="block space-y-1">
        <span class="text-base text-text-primary">{t("security.userPicker.filter")}</span>
        <input class="input-field min-h-12" value={filter()} onInput={(e) => setFilter(e.currentTarget.value)} />
      </label>
      <Show when={shown().length > 0} fallback={<p class="text-base text-text-secondary">{t("security.userPicker.none")}</p>}>
        <ul class="space-y-1">
          <For each={shown()}>
            {(f) => (
              <li>
                <button
                  type="button"
                  class="w-full min-h-12 px-3 rounded-lg text-left text-base text-text-primary hover:bg-white/5"
                  aria-label={t("security.userPicker.option", { name: f.display_name, username: f.username })}
                  onClick={() => props.onPick({ user_id: f.user_id, username: f.username, display_name: f.display_name })}
                >
                  {f.display_name} <span class="text-text-secondary">({f.username})</span>
                </button>
              </li>
            )}
          </For>
        </ul>
      </Show>
      <button
        type="button"
        class="min-h-12 px-4 rounded-lg border border-white/10 text-base text-text-primary"
        onClick={() => props.onCancel()}
      >
        {t("common.cancel")}
      </button>
    </div>
  );
};

export default UserPicker;
