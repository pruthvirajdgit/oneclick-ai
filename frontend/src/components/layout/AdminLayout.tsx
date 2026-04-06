import { NavLink, Outlet, Link } from "react-router-dom";
import {
  ArrowLeft,
  Users,
  Bot,
  BarChart3,
  HeartPulse,
  ShieldCheck,
} from "lucide-react";
import { cn } from "@/lib/utils";

const adminNav = [
  { to: "/admin/users", icon: Users, label: "Users" },
  { to: "/admin/agents", icon: Bot, label: "Agents" },
  { to: "/admin/analytics", icon: BarChart3, label: "Analytics" },
  { to: "/admin/health", icon: HeartPulse, label: "Health" },
];

export function AdminLayout() {
  return (
    <div className="flex h-screen bg-background">
      {/* Sidebar */}
      <aside className="hidden md:flex w-64 flex-col border-r border-border bg-card">
        <div className="flex items-center gap-2 px-6 py-5 border-b border-border">
          <div className="h-8 w-8 rounded-lg bg-[#8b5cf6] flex items-center justify-center">
            <ShieldCheck className="h-4 w-4 text-white" />
          </div>
          <span className="text-lg font-semibold text-foreground">
            Admin Panel
          </span>
        </div>

        <div className="px-3 pt-3">
          <Link
            to="/dashboard"
            className="flex items-center gap-2 rounded-lg px-3 py-2 text-sm font-medium text-muted-foreground hover:bg-muted hover:text-foreground transition-colors"
          >
            <ArrowLeft className="h-4 w-4" />
            Back to App
          </Link>
        </div>

        <nav className="flex-1 px-3 py-4 space-y-1">
          {adminNav.map(({ to, icon: Icon, label }) => (
            <NavLink
              key={to}
              to={to}
              className={({ isActive }) =>
                cn(
                  "flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm font-medium transition-colors",
                  isActive
                    ? "bg-[#8b5cf6]/10 text-[#8b5cf6]"
                    : "text-muted-foreground hover:bg-muted hover:text-foreground",
                )
              }
            >
              <Icon className="h-5 w-5" />
              {label}
            </NavLink>
          ))}
        </nav>
      </aside>

      {/* Main content */}
      <div className="flex flex-1 flex-col overflow-hidden">
        <header className="flex h-14 items-center border-b border-border bg-card px-6">
          <div className="md:hidden flex items-center gap-2">
            <div className="h-7 w-7 rounded-md bg-[#8b5cf6] flex items-center justify-center">
              <ShieldCheck className="h-3.5 w-3.5 text-white" />
            </div>
            <span className="font-semibold text-foreground">Admin</span>
          </div>
          <div className="hidden md:block" />
          <div className="ml-auto">
            <Link
              to="/dashboard"
              className="text-sm text-muted-foreground hover:text-foreground transition-colors"
            >
              ← Back to App
            </Link>
          </div>
        </header>

        <main className="flex-1 overflow-y-auto p-6">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
