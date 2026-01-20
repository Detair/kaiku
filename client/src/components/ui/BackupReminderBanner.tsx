/**
 * Backup Reminder Banner
 *
 * Shows a warning if user hasn't set up E2EE backup.
 */

import { Component } from "solid-js";
import { AlertTriangle, X } from "lucide-solid";

interface BackupReminderBannerProps {
  onSetup: () => void;
  onDismiss: () => void;
}

const BackupReminderBanner: Component<BackupReminderBannerProps> = (props) => {
  return (
    <div class="bg-yellow-500/10 border-b border-yellow-500/30 px-4 py-2 flex items-center gap-3">
      <AlertTriangle class="w-4 h-4 text-yellow-400 flex-shrink-0" />
      <p class="text-sm text-yellow-200 flex-1">
        Your encryption keys are not backed up.{" "}
        <button
          onClick={props.onSetup}
          class="underline hover:no-underline font-medium"
        >
          Set up now
        </button>
      </p>
      <button
        onClick={props.onDismiss}
        class="p-1 text-yellow-400 hover:text-yellow-200 transition-colors"
        title="Remind me later"
      >
        <X class="w-4 h-4" />
      </button>
    </div>
  );
};

export default BackupReminderBanner;
