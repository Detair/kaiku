/**
 * EventsModal — list a guild's upcoming scheduled events, RSVP, and (for
 * MANAGE_EVENTS) create new ones. Times display in the viewer's locale.
 */

import { Component, For, Show, createResource, createSignal, createUniqueId } from "solid-js";
import { Calendar, Users, Plus, X, MapPin } from "lucide-solid";
import {
  listGuildEvents,
  createGuildEvent,
  rsvpGuildEvent,
  clearGuildEventRsvp,
  type GuildEvent,
} from "@/lib/tauri";
import { t } from "@/i18n";
import { errorText } from "@/i18n/errors";
import Modal from "@/components/ui/Modal";
import Button from "@/components/ui/Button";

const EventsModal: Component<{
  guildId: string;
  canManage: boolean;
  onClose: () => void;
}> = (props) => {
  const [events, { refetch }] = createResource(
    () => props.guildId,
    (g) => listGuildEvents(g),
  );
  const [composing, setComposing] = createSignal(false);
  const [name, setName] = createSignal("");
  const [startsAt, setStartsAt] = createSignal("");
  const [location, setLocation] = createSignal("");
  const [error, setError] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);
  const titleId = createUniqueId();
  const nameId = createUniqueId();
  const startId = createUniqueId();
  const placeId = createUniqueId();

  const fmt = (iso: string) =>
    new Date(iso).toLocaleString(undefined, {
      weekday: "short",
      month: "short",
      day: "numeric",
      hour: "numeric",
      minute: "2-digit",
    });

  const submit = async (e: Event) => {
    e.preventDefault();
    if (!name().trim() || !startsAt() || busy()) return;
    setBusy(true);
    setError(null);
    try {
      await createGuildEvent(props.guildId, {
        name: name().trim(),
        starts_at: new Date(startsAt()).toISOString(),
        location: location().trim() || undefined,
      });
      setName("");
      setStartsAt("");
      setLocation("");
      setComposing(false);
      await refetch();
    } catch (err) {
      setError(errorText(err, "guilds.events.createFailed"));
    } finally {
      setBusy(false);
    }
  };

  const rsvp = async (ev: GuildEvent, response: "interested" | "going") => {
    try {
      if (ev.my_response === response) {
        await clearGuildEventRsvp(props.guildId, ev.id);
      } else {
        await rsvpGuildEvent(props.guildId, ev.id, response);
      }
      await refetch();
    } catch (err) {
      console.error("RSVP failed:", err);
    }
  };

  return (
    <Modal
      onClose={() => props.onClose()}
      labelledBy={titleId}
      class="w-full max-w-lg max-h-[80vh] rounded-xl flex flex-col overflow-hidden"
      style={{ "background-color": "var(--color-surface-base)" }}
      testId="events-dialog"
    >
      <header class="flex items-center justify-between p-4 border-b border-white/10">
        <h2 id={titleId} class="flex items-center gap-2 text-text-primary font-semibold">
          <Calendar class="w-5 h-5" aria-hidden="true" />
          {t("guilds.events.title")}
        </h2>
        <div class="flex items-center gap-2">
          <Show when={props.canManage}>
            <Button
              size="sm"
              onClick={() => setComposing((c) => !c)}
              aria-expanded={composing()}
            >
              <Plus class="w-4 h-4" aria-hidden="true" />
              {t("guilds.events.new")}
            </Button>
          </Show>
          <button
            type="button"
            class="p-1.5 tap-target rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10"
            onClick={() => props.onClose()}
            aria-label={t("common.close")}
          >
            <X class="w-5 h-5" aria-hidden="true" />
          </button>
        </div>
      </header>

      <div class="flex-1 overflow-y-auto p-4 space-y-3">
        <Show when={composing()}>
          <form
            onSubmit={submit}
            class="p-3 rounded-lg border border-white/10 space-y-2"
            style={{ "background-color": "var(--color-surface-layer1)" }}
          >
            <label for={nameId} class="block text-xs font-medium text-text-secondary">
              {t("guilds.events.name")}
            </label>
            <input
              id={nameId}
              class="input-field text-sm"
              type="text"
              maxlength={100}
              value={name()}
              onInput={(e) => setName(e.currentTarget.value)}
            />
            <label for={startId} class="block text-xs font-medium text-text-secondary">
              {t("guilds.events.start")}
            </label>
            <input
              id={startId}
              class="input-field text-sm"
              type="datetime-local"
              value={startsAt()}
              onInput={(e) => setStartsAt(e.currentTarget.value)}
            />
            <label for={placeId} class="block text-xs font-medium text-text-secondary">
              {t("guilds.events.place")}
            </label>
            <input
              id={placeId}
              class="input-field text-sm"
              type="text"
              value={location()}
              onInput={(e) => setLocation(e.currentTarget.value)}
            />
            <Show when={error()}>
              <div role="alert" class="text-xs text-error-text">{error()}</div>
            </Show>
            <div class="flex justify-end">
              <Button
                type="submit"
                size="sm"
                disabled={!name().trim() || !startsAt() || busy()}
                disabledReason={
                  !busy() && (!name().trim() || !startsAt()) && t("guilds.events.missing")
                }
              >
                {t("guilds.events.create")}
              </Button>
            </div>
          </form>
        </Show>

        <Show
          when={!events.loading}
          fallback={
            <div role="status" class="text-center py-8 text-text-secondary">
              {t("common.loading")}
            </div>
          }
        >
          <Show
            when={(events()?.length ?? 0) > 0}
            fallback={
              <div class="text-center py-10 text-text-secondary text-sm">
                {t("guilds.events.empty")}
              </div>
            }
          >
            <For each={events()}>
              {(ev) => (
                <div
                  class="p-3 rounded-lg border border-white/10"
                  style={{ "background-color": "var(--color-surface-layer1)" }}
                >
                  <div class="font-medium text-text-primary">{ev.name}</div>
                  <div class="text-xs text-text-secondary mt-0.5">{fmt(ev.starts_at)}</div>
                  <Show when={ev.location}>
                    <div class="text-xs text-text-secondary flex items-center gap-1 mt-0.5">
                      <MapPin class="w-3 h-3" aria-hidden="true" />
                      {ev.location}
                    </div>
                  </Show>
                  <div class="flex items-center gap-2 mt-2">
                    <button
                      type="button"
                      aria-pressed={ev.my_response === "going"}
                      class="px-2.5 py-1 rounded-lg text-xs font-medium transition-colors"
                      classList={{
                        "bg-accent-primary/20 text-text-primary": ev.my_response === "going",
                        "bg-[var(--color-surface-layer2)] text-text-secondary hover:text-text-primary":
                          ev.my_response !== "going",
                      }}
                      onClick={() => rsvp(ev, "going")}
                    >
                      {t("guilds.events.going", { count: ev.going_count })}
                    </button>
                    <button
                      type="button"
                      aria-pressed={ev.my_response === "interested"}
                      class="px-2.5 py-1 rounded-lg text-xs font-medium transition-colors"
                      classList={{
                        "bg-accent-primary/20 text-text-primary": ev.my_response === "interested",
                        "bg-[var(--color-surface-layer2)] text-text-secondary hover:text-text-primary":
                          ev.my_response !== "interested",
                      }}
                      onClick={() => rsvp(ev, "interested")}
                    >
                      {t("guilds.events.interested", { count: ev.interested_count })}
                    </button>
                    <span class="text-xs text-text-secondary flex items-center gap-1 ml-auto">
                      <Users class="w-3 h-3" aria-hidden="true" />
                      <span>
                        {t("guilds.events.total", { count: ev.going_count + ev.interested_count })}
                      </span>
                    </span>
                  </div>
                </div>
              )}
            </For>
          </Show>
        </Show>
      </div>
    </Modal>
  );
};

export default EventsModal;
