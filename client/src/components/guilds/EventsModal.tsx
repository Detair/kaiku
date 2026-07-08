/**
 * EventsModal — list a guild's upcoming scheduled events, RSVP, and (for
 * MANAGE_EVENTS) create new ones. Times display in the viewer's locale.
 */

import { Component, For, Show, createResource, createSignal } from "solid-js";
import { Calendar, Users, Plus, X, MapPin } from "lucide-solid";
import {
  listGuildEvents,
  createGuildEvent,
  rsvpGuildEvent,
  clearGuildEventRsvp,
  type GuildEvent,
} from "@/lib/tauri";

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
      setError(err instanceof Error ? err.message : "Failed to create event");
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
    <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4">
      <div
        class="w-full max-w-lg max-h-[80vh] rounded-xl flex flex-col overflow-hidden"
        style={{ "background-color": "var(--color-surface-base)" }}
      >
        <header class="flex items-center justify-between p-4 border-b border-white/10">
          <div class="flex items-center gap-2 text-text-primary font-semibold">
            <Calendar class="w-5 h-5" />
            Events
          </div>
          <div class="flex items-center gap-2">
            <Show when={props.canManage}>
              <button
                class="flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-accent-primary text-on-accent text-sm font-medium hover:bg-accent-primary/90"
                onClick={() => setComposing((c) => !c)}
              >
                <Plus class="w-4 h-4" />
                New
              </button>
            </Show>
            <button
              class="p-1.5 rounded-lg text-text-secondary hover:text-text-primary hover:bg-white/10"
              onClick={props.onClose}
            >
              <X class="w-5 h-5" />
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
              <input
                class="w-full px-3 py-2 rounded-lg text-sm text-text-primary bg-[var(--color-surface-layer2)] border border-white/10 outline-none"
                type="text"
                placeholder="Event name"
                maxlength={100}
                value={name()}
                onInput={(e) => setName(e.currentTarget.value)}
              />
              <input
                class="w-full px-3 py-2 rounded-lg text-sm text-text-primary bg-[var(--color-surface-layer2)] border border-white/10 outline-none"
                type="datetime-local"
                value={startsAt()}
                onInput={(e) => setStartsAt(e.currentTarget.value)}
              />
              <input
                class="w-full px-3 py-2 rounded-lg text-sm text-text-primary bg-[var(--color-surface-layer2)] border border-white/10 outline-none"
                type="text"
                placeholder="Location (optional)"
                value={location()}
                onInput={(e) => setLocation(e.currentTarget.value)}
              />
              <Show when={error()}>
                <div class="text-xs text-accent-danger">{error()}</div>
              </Show>
              <div class="flex justify-end">
                <button
                  type="submit"
                  disabled={!name().trim() || !startsAt() || busy()}
                  class="px-3 py-1.5 rounded-lg bg-accent-primary text-on-accent text-sm font-medium disabled:opacity-50"
                >
                  Create
                </button>
              </div>
            </form>
          </Show>

          <Show
            when={!events.loading}
            fallback={<div class="text-center py-8 text-text-secondary">Loading…</div>}
          >
            <Show
              when={(events()?.length ?? 0) > 0}
              fallback={
                <div class="text-center py-10 text-text-secondary text-sm">
                  No upcoming events.
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
                    <div class="text-xs text-text-secondary mt-0.5">
                      {fmt(ev.starts_at)}
                    </div>
                    <Show when={ev.location}>
                      <div class="text-xs text-text-secondary flex items-center gap-1 mt-0.5">
                        <MapPin class="w-3 h-3" />
                        {ev.location}
                      </div>
                    </Show>
                    <div class="flex items-center gap-2 mt-2">
                      <button
                        class="px-2.5 py-1 rounded-lg text-xs font-medium transition-colors"
                        classList={{
                          "bg-accent-primary/20 text-text-primary":
                            ev.my_response === "going",
                          "bg-[var(--color-surface-layer2)] text-text-secondary hover:text-text-primary":
                            ev.my_response !== "going",
                        }}
                        onClick={() => rsvp(ev, "going")}
                      >
                        Going · {ev.going_count}
                      </button>
                      <button
                        class="px-2.5 py-1 rounded-lg text-xs font-medium transition-colors"
                        classList={{
                          "bg-accent-primary/20 text-text-primary":
                            ev.my_response === "interested",
                          "bg-[var(--color-surface-layer2)] text-text-secondary hover:text-text-primary":
                            ev.my_response !== "interested",
                        }}
                        onClick={() => rsvp(ev, "interested")}
                      >
                        Interested · {ev.interested_count}
                      </button>
                      <span class="text-xs text-text-muted flex items-center gap-1 ml-auto">
                        <Users class="w-3 h-3" />
                        {ev.going_count + ev.interested_count}
                      </span>
                    </div>
                  </div>
                )}
              </For>
            </Show>
          </Show>
        </div>
      </div>
    </div>
  );
};

export default EventsModal;
