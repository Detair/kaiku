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
    <div class="bg-accent-warning/10 border-b border-accent-warning/30 px-4 py-2 flex items-center gap-3">
      <AlertTriangle class="w-4 h-4 text-accent-warning flex-shrink-0" />
      <p class="text-sm text-text-primary flex-1">
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
        class="p-1 text-text-secondary hover:text-text-primary transition-colors"
        title="Remind me later"
      >
        <X class="w-4 h-4" />
      </button>
    </div>
  );
};

export default BackupReminderBanner;
