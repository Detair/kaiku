/**
 * The signed-in app's account-security overlays (B.4, SM3, C27 prompts):
 * the chat-only banner, the open-request prompt and the migration prompt.
 * One lazy chunk, mounted by `Main`: none of it is needed to start the app
 * or to sign in (bundle budget).
 */

import { Component } from "solid-js";
import ProbationBanner from "@/components/auth/ProbationBanner";
import OpenRequestModal from "./OpenRequestModal";
import SecurityPromptBanner from "./SecurityPromptBanner";

const SecurityHosts: Component = () => (
  <>
    {/* Chat-only session: what it cannot do, and the way to a full one (B.4) */}
    <ProbationBanner />
    <OpenRequestModal />
    <SecurityPromptBanner />
  </>
);

export default SecurityHosts;
