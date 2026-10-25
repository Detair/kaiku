import { Component, onCleanup } from "solid-js";
import { A } from "@solidjs/router";
import { t } from "@/i18n";

const NotFound: Component = () => {
  document.title = `${t("auth.notFound.title")} | Kaiku`;
  onCleanup(() => { document.title = "Kaiku"; });

  return (
    <div class="flex items-center justify-center min-h-screen bg-background-primary">
      <div class="text-center p-8">
        <h1 class="text-6xl font-bold text-accent-primary mb-4">404</h1>
        <p class="text-xl text-text-primary mb-2">{t("auth.notFound.heading")}</p>
        <p class="text-text-secondary mb-6">
          {t("auth.notFound.body")}
        </p>
        <A href="/" class="btn-primary inline-flex items-center px-6 py-3">
          {t("common.goHome")}
        </A>
      </div>
    </div>
  );
};

export default NotFound;
