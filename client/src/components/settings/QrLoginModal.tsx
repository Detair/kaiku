/**
 * QR Login Modal
 *
 * Generates a QR code containing a kaiku:// deep link that the mobile app
 * can scan to authenticate without manual URL/credential entry.
 * The QR code includes a single-use token with a countdown timer.
 */

import { Component, createSignal, onMount, onCleanup, Show } from "solid-js";
import { X, Smartphone } from "lucide-solid";
import QRCode from "qrcode";
import { qrLoginCreate } from "@/lib/tauri";
import { showToast } from "@/components/ui/Toast";

interface QrLoginModalProps {
  serverUrl: string;
  onClose: () => void;
}

const QrLoginModal: Component<QrLoginModalProps> = (props) => {
  onMount(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") props.onClose();
    };
    document.addEventListener("keydown", handleKeyDown);
    onCleanup(() => document.removeEventListener("keydown", handleKeyDown));
  });

  const [qrDataUrl, setQrDataUrl] = createSignal<string | null>(null);
  const [isLoading, setIsLoading] = createSignal(false);
  const [error, setError] = createSignal("");
  const [secondsLeft, setSecondsLeft] = createSignal(0);

  let countdownInterval: number | undefined;

  const generateQr = async () => {
    setIsLoading(true);
    setError("");
    setQrDataUrl(null);

    try {
      const response = await qrLoginCreate();
      const uri = `kaiku://qr/login?server=${encodeURIComponent(props.serverUrl)}&token=${response.token}`;
      const dataUrl = await QRCode.toDataURL(uri, {
        width: 256,
        margin: 2,
      });
      setQrDataUrl(dataUrl);
      setSecondsLeft(response.expires_in);

      // Start countdown
      if (countdownInterval) clearInterval(countdownInterval);
      countdownInterval = window.setInterval(() => {
        setSecondsLeft((prev) => {
          if (prev <= 1) {
            clearInterval(countdownInterval);
            return 0;
          }
          return prev - 1;
        });
      }, 1000);
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setError(message);
      showToast({
        type: "error",
        title: "QR Code Failed",
        message: "Could not generate QR login code.",
        duration: 5000,
      });
    } finally {
      setIsLoading(false);
    }
  };

  // Generate on mount
  generateQr();

  onCleanup(() => {
    if (countdownInterval) clearInterval(countdownInterval);
  });

  return (
    <div
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      onClick={props.onClose}
    >
      <div
        class="bg-background-secondary rounded-xl shadow-2xl w-full max-w-sm mx-4"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div class="flex items-center justify-between p-4 border-b border-white/10">
          <div class="flex items-center gap-3">
            <Smartphone class="w-5 h-5 text-accent-primary" />
            <h2 class="text-lg font-semibold text-text-primary">
              Link Mobile Device
            </h2>
          </div>
          <button
            onClick={props.onClose}
            class="p-1.5 hover:bg-white/10 rounded-lg transition-colors text-text-secondary"
          >
            <X class="w-5 h-5" />
          </button>
        </div>

        {/* Content */}
        <div class="p-6 flex flex-col items-center gap-4">
          <p class="text-sm text-text-secondary text-center">
            Scan this code with the Kaiku mobile app to sign in
          </p>

          <Show
            when={!isLoading() && !error() && qrDataUrl()}
            fallback={
              <Show
                when={!error()}
                fallback={
                  <div class="text-center py-8">
                    <p class="text-sm text-red-400 mb-3">{error()}</p>
                    <button
                      onClick={generateQr}
                      class="px-4 py-2 bg-accent-primary hover:bg-accent-primary/80 rounded-lg transition-colors text-on-accent text-sm font-medium"
                    >
                      Try again
                    </button>
                  </div>
                }
              >
                <div class="w-64 h-64 flex items-center justify-center">
                  <span class="w-8 h-8 border-2 border-white/30 border-t-accent-primary rounded-full animate-spin" />
                </div>
              </Show>
            }
          >
            <div class="p-4 bg-white rounded-xl">
              <img
                src={qrDataUrl()!}
                alt="QR Login Code"
                class="w-56 h-56"
              />
            </div>

            <Show
              when={secondsLeft() > 0}
              fallback={
                <div class="text-center">
                  <p class="text-sm text-text-secondary mb-2">Code expired</p>
                  <button
                    onClick={generateQr}
                    class="px-4 py-2 bg-accent-primary hover:bg-accent-primary/80 rounded-lg transition-colors text-on-accent text-sm font-medium"
                  >
                    Generate new code
                  </button>
                </div>
              }
            >
              <p class="text-sm text-text-secondary">
                Expires in {secondsLeft()}s
              </p>
            </Show>
          </Show>
        </div>

        {/* Footer */}
        <div class="p-4 border-t border-white/10">
          <button
            onClick={props.onClose}
            class="w-full px-4 py-2 bg-white/10 hover:bg-white/20 rounded-lg transition-colors text-text-primary text-sm"
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
};

export default QrLoginModal;
