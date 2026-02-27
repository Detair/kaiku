/**
 * Backup Codes Display
 *
 * Reusable component for displaying MFA backup codes with copy-all and download buttons.
 */

import { Component, For } from "solid-js";
import { Copy, Download, AlertTriangle } from "lucide-solid";
import { showToast } from "@/components/ui/Toast";

interface BackupCodesDisplayProps {
  codes: string[];
}

const BackupCodesDisplay: Component<BackupCodesDisplayProps> = (props) => {
  const handleCopyAll = async () => {
    try {
      const text = props.codes.join("\n");
      await navigator.clipboard.writeText(text);
      showToast({
        type: "success",
        title: "Copied",
        message: "Backup codes copied to clipboard.",
        duration: 3000,
      });
    } catch {
      showToast({
        type: "error",
        title: "Copy Failed",
        message: "Could not copy to clipboard.",
        duration: 5000,
      });
    }
  };

  const handleDownload = () => {
    const text = [
      "VoiceChat MFA Backup Codes",
      "==========================",
      `Generated: ${new Date().toISOString()}`,
      "",
      "Each code can only be used once.",
      "Store these codes in a safe place.",
      "",
      ...props.codes.map((code, i) => `${i + 1}. ${code}`),
    ].join("\n");

    const blob = new Blob([text], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "voicechat-backup-codes.txt";
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);

    showToast({
      type: "success",
      title: "Downloaded",
      message: "Backup codes saved to file.",
      duration: 3000,
    });
  };

  return (
    <div class="space-y-4">
      <div class="flex items-start gap-3 p-3 bg-yellow-500/10 border border-yellow-500/30 rounded-lg">
        <AlertTriangle class="w-5 h-5 text-yellow-400 flex-shrink-0 mt-0.5" />
        <p class="text-sm text-yellow-200">
          Save these codes now. They will not be shown again. Each code can only
          be used once.
        </p>
      </div>

      <div class="grid grid-cols-2 gap-2">
        <For each={props.codes}>
          {(code) => (
            <div class="px-3 py-2 bg-white/5 rounded-lg font-mono text-sm text-text-primary text-center select-all">
              {code}
            </div>
          )}
        </For>
      </div>

      <div class="flex gap-2">
        <button
          onClick={handleCopyAll}
          class="flex-1 flex items-center justify-center gap-2 px-4 py-2 bg-white/10 hover:bg-white/20 rounded-lg transition-colors text-text-primary text-sm"
        >
          <Copy class="w-4 h-4" />
          Copy All
        </button>
        <button
          onClick={handleDownload}
          class="flex-1 flex items-center justify-center gap-2 px-4 py-2 bg-white/10 hover:bg-white/20 rounded-lg transition-colors text-text-primary text-sm"
        >
          <Download class="w-4 h-4" />
          Download
        </button>
      </div>
    </div>
  );
};

export default BackupCodesDisplay;
