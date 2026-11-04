/** One event's facts: time, client, place / IP (only where the reader may see them), method. */

import { Component, Show } from "solid-js";
import { t } from "@/i18n";
import { formatTimestamp } from "@/i18n/format";
import type { SecurityEvent } from "@/lib/types";

const SecurityEventDetails: Component<{ event: SecurityEvent }> = (props) => (
  <ul class="text-sm text-text-secondary space-y-0.5">
    <li>{t("security.notice.at", { time: formatTimestamp(props.event.created_at) })}</li>
    <Show when={props.event.client?.label}>
      {(label) => <li>{t("security.notice.from", { client: label() })}</li>}
    </Show>
    <Show when={props.event.place}>{(place) => <li>{t("security.notice.place", { place: place() })}</li>}</Show>
    <Show when={props.event.ip}>{(ip) => <li>{t("security.notice.ip", { ip: ip() })}</li>}</Show>
    <Show when={props.event.authenticator?.label}>
      {(label) => <li>{t("security.notice.with", { label: label() })}</li>}
    </Show>
  </ul>
);

export default SecurityEventDetails;
