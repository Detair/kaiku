/** AttemptEnded — why a sign-in attempt ended, with "Try again". */

import { Component } from "solid-js";
import { t, type MessageKey } from "@/i18n";
import type { EndedState, FlowVia } from "@/stores/loginFlow";

const TEXT: Record<EndedState, MessageKey> = {
  denied: "auth.ended.denied",
  reported: "auth.ended.reported",
  cancelled: "auth.ended.cancelled",
  expired: "auth.ended.expired",
};

export interface AttemptEndedProps {
  state: EndedState;
  via: FlowVia;
  onRetry: () => void;
  onBack: () => void;
}

const AttemptEnded: Component<AttemptEndedProps> = (props) => (
  <section class="space-y-5 text-center" role="status">
    <p class="text-base text-text-primary">{t(TEXT[props.state])}</p>
    <button type="button" class="btn-primary w-full min-h-12 text-base" onClick={() => props.onRetry()}>
      {t("common.tryAgain")}
    </button>
    <button
      type="button"
      class="w-full min-h-12 rounded-lg text-base text-text-secondary hover:text-text-primary"
      onClick={() => props.onBack()}
    >
      {t("auth.flow.back")}
    </button>
  </section>
);

export default AttemptEnded;
