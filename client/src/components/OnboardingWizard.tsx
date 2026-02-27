/**
 * OnboardingWizard - First-time user experience.
 *
 * Shows when preferences().onboarding_completed is false and
 * the admin setup wizard is not required. Guides new users through:
 * 1. Welcome (display name)
 * 2. Theme selection
 * 3. Mic setup (optional)
 * 4. Join a server (discovery or invite code)
 * 5. Done
 */

import {
  Component,
  createSignal,
  createEffect,
  on,
  onCleanup,
  Show,
  For,
  lazy,
  Suspense,
} from "solid-js";
import {
  Check,
  ChevronRight,
  ChevronLeft,
  Mic,
  Compass,
  Users,
} from "lucide-solid";
import { preferences, updatePreference } from "@/stores/preferences";
import { currentUser, updateUser } from "@/stores/auth";
import {
  setTheme,
  availableThemes,
  theme,
  type ThemeDefinition,
} from "@/stores/theme";
import { authState } from "@/stores/auth";
import { joinViaInviteCode } from "@/stores/guilds";
import { showToast } from "@/components/ui/Toast";
import type { DiscoverableGuild } from "@/lib/types";
import { discoverGuilds, joinDiscoverable } from "@/lib/tauri";

const MicTestPanel = lazy(() => import("@/components/voice/MicTestPanel"));

const TOTAL_STEPS = 5;

const OnboardingWizard: Component = () => {
  // Don't render if onboarding is complete, setup wizard is showing, or auth not initialized yet
  const shouldShow = () =>
    authState.isInitialized &&
    !preferences().onboarding_completed &&
    !authState.setupRequired;

  const [step, setStep] = createSignal(0);
  const [displayName, setDisplayName] = createSignal("");
  const [inviteCode, setInviteCode] = createSignal("");
  const [joiningInvite, setJoiningInvite] = createSignal(false);
  const [savingName, setSavingName] = createSignal(false);
  const [discoveryGuilds, setDiscoveryGuilds] = createSignal<
    DiscoverableGuild[]
  >([]);
  const [discoveryLoading, setDiscoveryLoading] = createSignal(false);
  const [discoveryError, setDiscoveryError] = createSignal(false);
  const [discoveryPermanent, setDiscoveryPermanent] = createSignal(false);
  const [joinTab, setJoinTab] = createSignal<"discover" | "invite">("discover");
  const [joinedIds, setJoinedIds] = createSignal<Set<string>>(new Set());

  let displayNameRef: HTMLInputElement | undefined;
  let dialogRef: HTMLDivElement | undefined;

  // Reset wizard state when re-shown (e.g. re-run from settings).
  // Also handles initial display name sync when user data loads.
  createEffect(() => {
    if (shouldShow()) {
      setStep(0);
      setDisplayName(currentUser()?.display_name ?? "");
    }
  });

  // Sync display name if user data loads asynchronously after wizard is already showing
  createEffect(
    on(
      () => currentUser()?.display_name,
      (name) => {
        if (name && step() === 0 && displayName() === "") {
          setDisplayName(name);
        }
      },
      { defer: true },
    ),
  );

  // Auto-focus first interactive element on step transitions (defer: skip initial)
  createEffect(
    on(
      step,
      (currentStep) => {
        if (currentStep === 0) {
          queueMicrotask(() => displayNameRef?.focus());
        }
      },
      { defer: true },
    ),
  );

  // Focus trap: constrain Tab cycling within the dialog (aria-modal="true" contract)
  const handleFocusTrap = (e: KeyboardEvent) => {
    if (e.key !== "Tab" || !dialogRef) return;
    const focusable = dialogRef.querySelectorAll<HTMLElement>(
      'button:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])',
    );
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  };

  createEffect(() => {
    if (shouldShow()) {
      document.addEventListener("keydown", handleFocusTrap);
      onCleanup(() => document.removeEventListener("keydown", handleFocusTrap));
    }
  });

  const complete = () => {
    updatePreference("onboarding_completed", true);
  };

  const next = () => {
    if (step() < TOTAL_STEPS - 1) {
      // Load discovery guilds when entering the join step
      if (step() + 1 === 3) {
        loadDiscoveryGuilds();
      }
      setStep(step() + 1);
    }
  };

  const back = () => {
    if (step() > 0) setStep(step() - 1);
  };

  const handleSaveDisplayName = async () => {
    if (savingName()) return;
    const name = displayName().trim();
    if (name && name !== currentUser()?.display_name) {
      setSavingName(true);
      try {
        const { fetchApi } = await import("@/lib/tauri");
        await fetchApi("/auth/me", {
          method: "POST",
          body: { display_name: name },
        });
        updateUser({ display_name: name });
      } catch (err: unknown) {
        console.error("Failed to save display name:", err);
        showToast({
          type: "warning",
          title: "Could Not Save Name",
          message:
            "Your display name wasn't saved. You can update it later in settings.",
          duration: 8000,
        });
        setSavingName(false);
        return; // Stay on this step so the user can retry or skip
      }
      setSavingName(false);
    }
    next();
  };

  const loadDiscoveryGuilds = async () => {
    setDiscoveryLoading(true);
    setDiscoveryError(false);
    setDiscoveryPermanent(false);
    try {
      const result = await discoverGuilds({
        sort: "members",
        limit: 6,
        offset: 0,
      });
      setDiscoveryGuilds(result.guilds);
    } catch (err: unknown) {
      console.error("Failed to load discovery guilds:", err);
      const isDisabled =
        err instanceof Error && err.message.includes("DISCOVERY_DISABLED");
      setDiscoveryPermanent(isDisabled);
      setDiscoveryError(true);
    } finally {
      setDiscoveryLoading(false);
    }
  };

  const handleJoinDiscoverable = async (guildId: string): Promise<boolean> => {
    try {
      const result = await joinDiscoverable(guildId);
      if (result.already_member) {
        showToast({
          type: "info",
          title: "Already a Member",
          message: `You're already in ${result.guild_name}.`,
        });
      } else {
        showToast({
          type: "success",
          title: "Joined!",
          message: `You've joined ${result.guild_name}.`,
        });
        // Refresh guild list so sidebar shows the new guild after wizard closes
        try {
          const { loadGuilds } = await import("@/stores/guilds");
          await loadGuilds();
        } catch (refreshErr) {
          console.error(
            "Failed to refresh guild list after onboarding join:",
            refreshErr,
          );
        }
      }
      setJoinedIds((prev) => new Set([...prev, guildId]));
      return true;
    } catch (err: unknown) {
      console.error("Failed to join discoverable guild:", err);
      showToast({
        type: "error",
        title: "Join Failed",
        message: "Could not join this server.",
      });
      return false;
    }
  };

  const handleJoinInvite = async () => {
    const code = inviteCode().trim();
    if (!code) return;
    setJoiningInvite(true);
    try {
      await joinViaInviteCode(code);
      setInviteCode("");
    } catch (err: unknown) {
      console.error("Failed to join via invite code:", err);
      showToast({
        type: "error",
        title: "Invalid Invite",
        message: "Could not join with this invite code.",
      });
    } finally {
      setJoiningInvite(false);
    }
  };

  return (
    <Show when={shouldShow()}>
      <div
        class="fixed inset-0 bg-black/80 flex items-center justify-center z-50"
        role="dialog"
        aria-modal="true"
        aria-label="Onboarding wizard"
      >
        <div
          ref={dialogRef}
          class="w-[36rem] max-h-[85vh] rounded-xl border border-white/10 shadow-2xl flex flex-col overflow-hidden"
          style="background-color: var(--color-surface-layer2)"
        >
          {/* Progress dots */}
          <div
            class="flex justify-center gap-2 pt-5 pb-2"
            role="progressbar"
            aria-valuenow={step() + 1}
            aria-valuemin={1}
            aria-valuemax={TOTAL_STEPS}
            aria-label={`Step ${step() + 1} of ${TOTAL_STEPS}`}
          >
            <For each={Array.from({ length: TOTAL_STEPS })}>
              {(_, i) => (
                <div
                  class="w-2 h-2 rounded-full transition-colors"
                  classList={{
                    "bg-accent-primary": i() === step(),
                    "bg-white/20": i() !== step(),
                  }}
                />
              )}
            </For>
          </div>

          {/* Step content */}
          <div class="flex-1 overflow-y-auto px-8 py-4">
            {/* Step 0: Welcome */}
            <Show when={step() === 0}>
              <div class="text-center mb-6">
                <h2 class="text-2xl font-bold text-text-primary">
                  Welcome to Canis
                </h2>
                <p class="text-sm text-text-secondary mt-2">
                  Let's get you set up in just a few steps.
                </p>
              </div>
              <div>
                <label
                  for="onboarding-display-name"
                  class="block text-sm font-medium text-text-secondary mb-2"
                >
                  Display Name
                </label>
                <input
                  id="onboarding-display-name"
                  ref={displayNameRef}
                  type="text"
                  value={displayName()}
                  onInput={(e) => setDisplayName(e.currentTarget.value)}
                  placeholder="How should others see you?"
                  maxLength={32}
                  class="w-full px-4 py-3 text-sm rounded-lg bg-surface-layer1 border border-white/10 text-text-primary placeholder-text-secondary focus:outline-none focus:border-accent-primary/50"
                />
                <p class="text-xs text-text-secondary mt-2">
                  You can change this later in settings.
                </p>
              </div>
            </Show>

            {/* Step 1: Theme */}
            <Show when={step() === 1}>
              <div class="text-center mb-6">
                <h2 class="text-xl font-bold text-text-primary">
                  Pick a Theme
                </h2>
                <p class="text-sm text-text-secondary mt-1">
                  Choose how Canis looks. You can change this anytime.
                </p>
              </div>
              <div class="grid grid-cols-2 gap-3">
                <For each={availableThemes}>
                  {(t: ThemeDefinition) => (
                    <button
                      onClick={() => setTheme(t.id)}
                      class="text-left p-3 rounded-xl border-2 transition-all"
                      classList={{
                        "border-accent-primary bg-accent-primary/10":
                          theme() === t.id,
                        "border-white/10 hover:border-accent-primary/50":
                          theme() !== t.id,
                      }}
                    >
                      <div class="flex items-center gap-2 mb-1">
                        <div class="flex gap-1">
                          <div
                            class="w-3 h-3 rounded-full border border-white/20"
                            style={{ "background-color": t.preview.surface }}
                          />
                          <div
                            class="w-3 h-3 rounded-full border border-white/20"
                            style={{ "background-color": t.preview.accent }}
                          />
                          <div
                            class="w-3 h-3 rounded-full border border-white/20"
                            style={{ "background-color": t.preview.text }}
                          />
                        </div>
                        <Show when={theme() === t.id}>
                          <Check class="w-3.5 h-3.5 text-accent-primary ml-auto" />
                        </Show>
                      </div>
                      <div class="text-sm font-semibold text-text-primary">
                        {t.name}
                      </div>
                      <div class="text-xs text-text-secondary">
                        {t.description}
                      </div>
                    </button>
                  )}
                </For>
              </div>
            </Show>

            {/* Step 2: Mic Setup */}
            <Show when={step() === 2}>
              <div class="text-center mb-4">
                <Mic class="w-8 h-8 text-accent-primary mx-auto mb-2" />
                <h2 class="text-xl font-bold text-text-primary">Mic Setup</h2>
                <p class="text-sm text-text-secondary mt-1">
                  Test your microphone and speakers. You can skip this step.
                </p>
              </div>
              <Suspense
                fallback={
                  <div class="h-40 flex items-center justify-center text-text-secondary text-sm">
                    Loading...
                  </div>
                }
              >
                <MicTestPanel compact />
              </Suspense>
            </Show>

            {/* Step 3: Join a Server */}
            <Show when={step() === 3}>
              <div class="text-center mb-4">
                <Compass class="w-8 h-8 text-accent-primary mx-auto mb-2" />
                <h2 class="text-xl font-bold text-text-primary">
                  Join a Server
                </h2>
                <p class="text-sm text-text-secondary mt-1">
                  Find a community or enter an invite code.
                </p>
              </div>

              {/* Tab switcher */}
              <div
                class="flex rounded-lg border border-white/10 overflow-hidden text-xs mb-4"
                role="tablist"
                aria-label="Join method"
              >
                <button
                  id="tab-discover"
                  role="tab"
                  aria-selected={joinTab() === "discover"}
                  aria-controls="tabpanel-discover"
                  onClick={() => setJoinTab("discover")}
                  class="flex-1 px-3 py-2 transition-colors"
                  classList={{
                    "bg-accent-primary text-white": joinTab() === "discover",
                    "bg-surface-layer1 text-text-secondary hover:text-text-primary":
                      joinTab() !== "discover",
                  }}
                >
                  Discover
                </button>
                <button
                  id="tab-invite"
                  role="tab"
                  aria-selected={joinTab() === "invite"}
                  aria-controls="tabpanel-invite"
                  onClick={() => setJoinTab("invite")}
                  class="flex-1 px-3 py-2 transition-colors"
                  classList={{
                    "bg-accent-primary text-white": joinTab() === "invite",
                    "bg-surface-layer1 text-text-secondary hover:text-text-primary":
                      joinTab() !== "invite",
                  }}
                >
                  Invite Code
                </button>
              </div>

              {/* Discover tab */}
              <Show when={joinTab() === "discover"}>
                <div
                  id="tabpanel-discover"
                  role="tabpanel"
                  aria-labelledby="tab-discover"
                >
                  <Show when={discoveryLoading()}>
                    <div class="grid grid-cols-2 gap-2">
                      <For each={Array.from({ length: 4 })}>
                        {() => (
                          <div class="h-20 rounded-lg bg-surface-layer1 animate-pulse" />
                        )}
                      </For>
                    </div>
                  </Show>
                  <Show when={!discoveryLoading() && discoveryError()}>
                    <div class="text-center py-8 text-text-secondary text-sm">
                      <p>
                        {discoveryPermanent()
                          ? "Guild discovery is not enabled on this server."
                          : "Could not load servers. Check your connection."}
                      </p>
                      <Show when={!discoveryPermanent()}>
                        <button
                          onClick={loadDiscoveryGuilds}
                          class="mt-2 px-3 py-1 text-xs bg-accent-primary text-white rounded-lg hover:bg-accent-hover"
                        >
                          Retry
                        </button>
                      </Show>
                    </div>
                  </Show>
                  <Show
                    when={
                      !discoveryLoading() &&
                      !discoveryError() &&
                      discoveryGuilds().length === 0
                    }
                  >
                    <div class="text-center py-8 text-text-secondary text-sm">
                      No discoverable servers yet. Try using an invite code
                      instead.
                    </div>
                  </Show>
                  <Show
                    when={
                      !discoveryLoading() &&
                      !discoveryError() &&
                      discoveryGuilds().length > 0
                    }
                  >
                    <div class="grid grid-cols-2 gap-2">
                      <For each={discoveryGuilds()}>
                        {(guild) => {
                          const initials = guild.name
                            .split(" ")
                            .map((w) => w[0])
                            .join("")
                            .toUpperCase()
                            .slice(0, 2);
                          const isJoined = () => joinedIds().has(guild.id);

                          return (
                            <div class="flex items-center gap-2 p-2.5 rounded-lg bg-surface-layer1 border border-white/5">
                              <div class="w-8 h-8 rounded-lg bg-surface-layer2 flex items-center justify-center overflow-hidden flex-shrink-0">
                                <Show
                                  when={guild.icon_url}
                                  fallback={
                                    <span class="text-[10px] font-bold text-text-primary">
                                      {initials}
                                    </span>
                                  }
                                >
                                  <img
                                    src={guild.icon_url!}
                                    alt=""
                                    class="w-full h-full object-cover"
                                  />
                                </Show>
                              </div>
                              <div class="flex-1 min-w-0">
                                <div class="text-xs font-semibold text-text-primary truncate">
                                  {guild.name}
                                </div>
                                <div class="flex items-center gap-1 text-[10px] text-text-secondary">
                                  <Users class="w-2.5 h-2.5" />
                                  {guild.member_count.toLocaleString()}
                                </div>
                              </div>
                              <button
                                onClick={() => handleJoinDiscoverable(guild.id)}
                                disabled={isJoined()}
                                class="px-2 py-1 text-[10px] font-medium rounded transition-colors flex-shrink-0"
                                classList={{
                                  "bg-accent-primary text-white hover:bg-accent-hover":
                                    !isJoined(),
                                  "bg-white/10 text-text-secondary cursor-default":
                                    isJoined(),
                                }}
                              >
                                {isJoined() ? "Joined" : "Join"}
                              </button>
                            </div>
                          );
                        }}
                      </For>
                    </div>
                  </Show>
                </div>
              </Show>

              {/* Invite code tab */}
              <Show when={joinTab() === "invite"}>
                <div
                  id="tabpanel-invite"
                  role="tabpanel"
                  aria-labelledby="tab-invite"
                  class="space-y-3"
                >
                  <input
                    type="text"
                    value={inviteCode()}
                    onInput={(e) => setInviteCode(e.currentTarget.value)}
                    placeholder="Enter invite code (e.g. AbCdEfGh)"
                    maxLength={8}
                    class="w-full px-4 py-3 text-sm rounded-lg bg-surface-layer1 border border-white/10 text-text-primary placeholder-text-secondary focus:outline-none focus:border-accent-primary/50"
                    onKeyDown={(e) => {
                      if (e.key === "Enter") handleJoinInvite();
                    }}
                  />
                  <button
                    onClick={handleJoinInvite}
                    disabled={!inviteCode().trim() || joiningInvite()}
                    class="w-full py-2.5 text-sm font-medium rounded-lg bg-accent-primary text-white hover:bg-accent-hover disabled:opacity-50 transition-colors"
                  >
                    {joiningInvite() ? "Joining..." : "Join Server"}
                  </button>
                </div>
              </Show>
            </Show>

            {/* Step 4: Done */}
            <Show when={step() === 4}>
              <div class="text-center py-6">
                <div class="w-16 h-16 rounded-full bg-accent-primary/20 flex items-center justify-center mx-auto mb-4">
                  <Check class="w-8 h-8 text-accent-primary" />
                </div>
                <h2 class="text-2xl font-bold text-text-primary">
                  You're All Set!
                </h2>
                <p class="text-sm text-text-secondary mt-2 max-w-xs mx-auto">
                  Welcome aboard, {currentUser()?.display_name ?? "friend"}.
                  Explore servers, chat with friends, and join voice channels.
                </p>
              </div>
            </Show>
          </div>

          {/* Footer navigation */}
          <div class="flex items-center justify-between px-8 py-4 border-t border-white/5">
            <div>
              <Show when={step() > 0 && step() < TOTAL_STEPS - 1}>
                <button
                  onClick={back}
                  class="flex items-center gap-1 text-sm text-text-secondary hover:text-text-primary transition-colors"
                >
                  <ChevronLeft class="w-4 h-4" />
                  Back
                </button>
              </Show>
            </div>

            <div class="flex items-center gap-3">
              {/* Skip button (not on welcome or done) */}
              <Show when={step() > 0 && step() < TOTAL_STEPS - 1}>
                <button
                  onClick={next}
                  class="text-sm text-text-secondary hover:text-text-primary transition-colors"
                >
                  Skip
                </button>
              </Show>

              {/* Next / Get Started button */}
              <Show
                when={step() < TOTAL_STEPS - 1}
                fallback={
                  <button
                    onClick={complete}
                    class="flex items-center gap-1.5 px-5 py-2 text-sm font-medium rounded-lg bg-accent-primary text-white hover:bg-accent-hover transition-colors"
                  >
                    Get Started
                    <ChevronRight class="w-4 h-4" />
                  </button>
                }
              >
                <button
                  onClick={() =>
                    step() === 0 ? handleSaveDisplayName() : next()
                  }
                  disabled={savingName()}
                  class="flex items-center gap-1.5 px-5 py-2 text-sm font-medium rounded-lg bg-accent-primary text-white hover:bg-accent-hover disabled:opacity-50 transition-colors"
                >
                  {savingName()
                    ? "Saving..."
                    : step() === 0
                      ? "Continue"
                      : "Next"}
                  <ChevronRight class="w-4 h-4" />
                </button>
              </Show>
            </div>
          </div>
        </div>
      </div>
    </Show>
  );
};

export default OnboardingWizard;
