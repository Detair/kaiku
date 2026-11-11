/**
 * AdminSidebar - Navigation sidebar for admin dashboard
 *
 * Provides navigation between the admin dashboard panels:
 * - Overview: Stats and quick actions
 * - Users: User management
 * - Guilds: Guild management
 * - Audit Log: Activity history
 * - Settings: Auth methods, OIDC providers, registration policy
 */

import { Component, For } from "solid-js";
import { t, type MessageKey } from "@/i18n";
import {
  LayoutDashboard,
  Users,
  Building2,
  ScrollText,
  Settings,
  Flag,
  Activity,
  BookOpen,
} from "lucide-solid";

export type AdminPanel =
  | "overview"
  | "users"
  | "guilds"
  | "platform-pages"
  | "reports"
  | "audit-log"
  | "command-center"
  | "settings";

interface AdminSidebarProps {
  activePanel: AdminPanel;
  onSelectPanel: (panel: AdminPanel) => void;
}

const AdminSidebar: Component<AdminSidebarProps> = (props) => {
  const items: {
    id: AdminPanel;
    label: MessageKey;
    icon: typeof LayoutDashboard;
  }[] = [
    { id: "overview", label: "admin.nav.overview", icon: LayoutDashboard },
    { id: "users", label: "admin.nav.users", icon: Users },
    { id: "guilds", label: "admin.nav.servers", icon: Building2 },
    { id: "platform-pages", label: "admin.nav.platformPages", icon: BookOpen },
    { id: "reports", label: "admin.nav.reports", icon: Flag },
    { id: "audit-log", label: "admin.nav.auditLog", icon: ScrollText },
    { id: "command-center", label: "admin.nav.commandCenter", icon: Activity },
    { id: "settings", label: "admin.nav.settings", icon: Settings },
  ];

  return (
    <div class="min-w-48 flex-shrink-0 border-r border-white/10 p-3 space-y-1">
      <For each={items}>
        {(item) => (
          <button
            data-testid={`admin-tab-${item.id}`}
            onClick={() => props.onSelectPanel(item.id)}
            aria-current={props.activePanel === item.id ? "page" : undefined}
            class="w-full flex items-center gap-3 px-3 py-2 rounded-lg text-sm font-medium transition-colors"
            classList={{
              "bg-accent-primary/20 text-text-primary":
                props.activePanel === item.id,
              "text-text-secondary hover:text-text-primary hover:bg-white/5":
                props.activePanel !== item.id,
            }}
          >
            <item.icon class="w-4 h-4 shrink-0" />
            {t(item.label)}
          </button>
        )}
      </For>
    </div>
  );
};

export default AdminSidebar;
