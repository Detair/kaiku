/**
 * Microphone Test Modal
 *
 * Wraps MicTestPanel in the standard dialog frame (modal layer, Escape,
 * focus trap and return, `role="dialog"`).
 */

import DialogShell from "@/components/auth/DialogShell";
import MicTestPanel from "./MicTestPanel";
import { t } from "@/i18n";

interface Props {
  onClose: () => void;
}

function MicrophoneTest(props: Props) {
  return (
    <DialogShell title={t("voice.micTest.title")} onClose={props.onClose} testId="microphone-test">
      <MicTestPanel />
    </DialogShell>
  );
}

export default MicrophoneTest;
