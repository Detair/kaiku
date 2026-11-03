/** "This sign-in is open for N more minutes." — ticks every 15 s. */

import { Component, createSignal, onCleanup } from "solid-js";
import { plural } from "@/i18n";

const MinutesLeft: Component<{ expiresAt: string }> = (props) => {
  const [now, setNow] = createSignal(Date.now());
  const timer = setInterval(() => setNow(Date.now()), 15_000);
  onCleanup(() => clearInterval(timer));
  const minutes = () => {
    const at = Date.parse(props.expiresAt);
    if (Number.isNaN(at)) return 0;
    return Math.max(0, Math.ceil((at - now()) / 60_000));
  };
  return (
    <p class="text-sm text-text-secondary text-center">
      {plural(minutes(), { one: "auth.phone.minutesLeft.one", other: "auth.phone.minutesLeft.other" })}
    </p>
  );
};

export default MinutesLeft;
