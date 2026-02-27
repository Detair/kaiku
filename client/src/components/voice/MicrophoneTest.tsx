/**
 * Microphone Test Modal
 *
 * Wraps MicTestPanel in a modal overlay for standalone use.
 */

import MicTestPanel from "./MicTestPanel";

interface Props {
  onClose: () => void;
}

function MicrophoneTest(props: Props) {
  return (
    <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div
        class="rounded-xl shadow-xl max-w-md w-full mx-4 border border-white/10"
        style="background-color: var(--color-surface-layer2)"
      >
        {/* Header */}
        <div class="flex items-center justify-between p-4 border-b border-white/5">
          <h3 class="text-lg font-semibold text-text-primary">
            Microphone Test
          </h3>
          <button
            onClick={props.onClose}
            class="text-text-secondary hover:text-text-primary transition-colors"
          >
            &times;
          </button>
        </div>

        {/* Content */}
        <div class="p-4">
          <MicTestPanel />
        </div>
      </div>
    </div>
  );
}

export default MicrophoneTest;
