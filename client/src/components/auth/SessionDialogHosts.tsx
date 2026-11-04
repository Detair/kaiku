/**
 * The session dialogs mounted once in `App.tsx`: full session required,
 * step-up confirmation and "sign out other sessions". One lazy chunk,
 * loaded right after startup and kept off the startup path (bundle budget).
 * Their open state lives in eager modules (`lib/fullSessionRequired`,
 * `lib/stepUp`, `stores/security`), so nothing raised before the chunk has
 * loaded is lost: it shows once the chunk is there.
 */

import { Component } from "solid-js";
import SignOutOthersPrompt from "@/components/settings/security/SignOutOthersPrompt";
import FullSessionRequiredDialog from "./FullSessionRequiredDialog";
import StepUpHost from "./StepUpDialog";

const SessionDialogHosts: Component = () => (
  <>
    <FullSessionRequiredDialog />
    <StepUpHost />
    <SignOutOthersPrompt />
  </>
);

export default SessionDialogHosts;
