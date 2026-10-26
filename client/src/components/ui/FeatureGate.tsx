/**
 * FeatureGate - renders children only while an optional server feature is on.
 * `whileUnknown` decides what happens before the flags have loaded.
 */

import { Component, JSX, Show } from "solid-js";
import {
  isFeatureEnabled,
  isFeatureOff,
  type OptionalFeatureKey,
} from "@/stores/serverFeatures";

const FeatureGate: Component<{
  feature: OptionalFeatureKey;
  /** Show the children while the flags are not loaded yet (default: hide). */
  whileUnknown?: boolean;
  children: JSX.Element;
}> = (props) => (
  <Show
    when={props.whileUnknown ? !isFeatureOff(props.feature) : isFeatureEnabled(props.feature)}
  >
    {props.children}
  </Show>
);

export default FeatureGate;
