import { useCallback, useEffect, useState } from "react";
import { NavLink } from "react-router-dom";
import {
  LayoutDashboard,
  MessageSquare,
  CalendarClock,
  BarChart3,
  Bell,
  ShieldCheck,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { api } from "@/lib/api";

interface Notification {
  id: number;
  read: boolean;
}

const navItems = [
  { to: "/dashboard", icon: LayoutDashboard, label: "Dashboard" },
  { to: "/chat", icon: MessageSquare, label: "Chat" },
  { to: "/schedules", icon: CalendarClock, label: "Schedules" },
  { to: "/usage", icon: BarChart3, label: "Usage" },
  { to: "/notifications", icon: Bell, label: "Notifications" },
];

export function Sidebar() {
  const [unreadCount, setUnreadCount] = useState(0);

  const fetchUnread = useCallback(async () => {
    try {
      const data = await api.get<Notification[]>("/notifications");
      setUnreadCount(data.filter((n) => !n.read).length);
    } catch {
      // silently fail — badge is non-critical
    }
  }, []);

  useEffect(() => {
    fetchUnread();
    const id = setInterval(fetchUnread, 60_000);
    return () => clearInterval(id);
  }, [fetchUnread]);

  return (
    <aside className="hidden md:flex w-64 flex-col border-r border-border bg-card">
      <div className="flex items-center gap-2 px-6 py-5 border-b border-border">
        <div className="h-8 w-8 rounded-lg bg-primary flex items-center justify-center">
          <span className="text-primary-foreground font-bold text-sm">O</span>
        </div>
        <span className="text-lg font-semibold text-foreground">
          OneClick.ai
        </span>
      </div>

      <nav className="flex-1 px-3 py-4 space-y-1">
        {navItems.map(({ to, icon: Icon, label }) => (
          <NavLink
            key={to}
            to={to}
            className={({ isActive }) =>
              cn(
                "flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm font-medium transition-colors",
                isActive
                  ? "bg-primary/10 text-primary"
                  : "text-muted-foreground hover:bg-muted hover:text-foreground"
              )
            }
          >
            <Icon className="h-5 w-5" />
            {label}
            {to === "/notifications" && unreadCount > 0 && (
              <span className="ml-auto flex h-5 min-w-5 items-center justify-center rounded-full bg-red-500 px-1.5 text-[11px] font-semibold text-white">
                {unreadCount > 99 ? "99+" : unreadCount}
              </span>
            )}
          </NavLink>
        ))}
      </nav>

      <div className="px-3 pb-4 border-t border-border pt-3">
        <NavLink
          to="/admin"
          className={({ isActive }) =>
            cn(
              "flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm font-medium transition-colors",
              isActive
                ? "bg-[#8b5cf6]/10 text-[#8b5cf6]"
                : "text-muted-foreground hover:bg-muted hover:text-foreground"
            )
          }
        >
          <ShieldCheck className="h-5 w-5" />
          Admin
        </NavLink>
      </div>
    </aside>
  );
}
