/**
 * InviteJoin - Handle invite link URLs
 *
 * Accepts invite code from URL, joins the server, redirects to it.
 */

import { Component, createSignal, onMount, Show } from "solid-js";
import { useParams, useNavigate } from "@solidjs/router";
import { joinViaInviteCode } from "@/stores/guilds";
import { authState } from "@/stores/auth";
import { t } from "@/i18n";
import { errorText } from "@/i18n/errors";

const InviteJoin: Component = () => {
  const params = useParams<{ code: string }>();
  const navigate = useNavigate();

  const [status, setStatus] = createSignal<"loading" | "success" | "error">(
    "loading",
  );
  const [errorMessage, setErrorMessage] = createSignal("");

  onMount(async () => {
    // Check if user is logged in
    if (!authState.user) {
      // Redirect to login with return URL
      navigate(`/login?returnUrl=${encodeURIComponent(`/invite/${params.code}`)}`);
      return;
    }

    try {
      await joinViaInviteCode(params.code);
      setStatus("success");
      // The joinViaInviteCode function already navigates to the guild
    } catch (err) {
      setStatus("error");
      setErrorMessage(errorText(err, "auth.invite.failed"));
    }
  });

  return (
    <div
      class="h-screen flex items-center justify-center"
      style={{"background-color":"var(--color-surface-base)"}}
    >
      <div
        class="text-center p-8 rounded-2xl border border-white/10 max-w-md"
        style={{"background-color":"var(--color-surface-layer1)"}}
      >
        <Show when={status() === "loading"}>
          <div class="text-text-primary text-lg mb-2">{t("auth.invite.joining")}</div>
          <div class="text-text-secondary">{t("auth.invite.wait")}</div>
        </Show>

        <Show when={status() === "success"}>
          <div class="text-accent-primary text-lg mb-2">{t("auth.invite.success")}</div>
          <div class="text-text-secondary">
            {t("auth.invite.joined")}
          </div>
        </Show>

        <Show when={status() === "error"}>
          <div class="text-accent-danger text-lg mb-2">{t("auth.invite.failedTitle")}</div>
          <div class="text-text-secondary mb-4">{errorMessage()}</div>
          <button
            onClick={() => navigate("/")}
            class="px-4 py-2 bg-accent-primary text-on-accent rounded-lg hover:opacity-90"
          >
            {t("common.goHome")}
          </button>
        </Show>
      </div>
    </div>
  );
};

export default InviteJoin;
