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
import { LayoutDashboard, Users, Building2, ScrollText, Settings } from "lucide-solid";

export type AdminPanel = "overview" | "users" | "guilds" | "audit-log" | "settings";

interface AdminSidebarProps {
  activePanel: AdminPanel;
  onSelectPanel: (panel: AdminPanel) => void;
}

const AdminSidebar: Component<AdminSidebarProps> = (props) => {
  const items: { id: AdminPanel; label: string; icon: typeof LayoutDashboard }[] = [
    { id: "overview", label: "Overview", icon: LayoutDashboard },
    { id: "users", label: "Users", icon: Users },
    { id: "guilds", label: "Guilds", icon: Building2 },
    { id: "audit-log", label: "Audit Log", icon: ScrollText },
    { id: "settings", label: "Settings", icon: Settings },
  ];

  return (
    <div class="w-48 flex-shrink-0 border-r border-white/10 p-3 space-y-1">
      <For each={items}>
        {(item) => (
          <button
            onClick={() => props.onSelectPanel(item.id)}
            class="w-full flex items-center gap-3 px-3 py-2 rounded-lg text-sm font-medium transition-colors"
            classList={{
              "bg-accent-primary/20 text-text-primary": props.activePanel === item.id,
              "text-text-secondary hover:text-text-primary hover:bg-white/5": props.activePanel !== item.id,
            }}
          >
            <item.icon
              class="w-4 h-4"
              style={{ color: props.activePanel === item.id ? "#FFFFFF" : undefined }}
            />
            {item.label}
          </button>
        )}
      </For>
    </div>
  );
};

export default AdminSidebar;
