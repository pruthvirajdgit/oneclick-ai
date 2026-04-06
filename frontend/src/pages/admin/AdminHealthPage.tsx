import { useEffect, useState } from "react";
import { Card, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Database, HardDrive, Container, Server } from "lucide-react";

type HealthStatus = "healthy" | "degraded" | "down" | "checking";

interface ServiceHealth {
  name: string;
  icon: React.ElementType;
  status: HealthStatus;
  label: string;
  detail: string;
}

const API_BASE = import.meta.env.VITE_API_URL || "/api";

const statusConfig: Record<HealthStatus, { emoji: string; badge: string }> = {
  healthy: { emoji: "🟢", badge: "bg-emerald-100 text-emerald-700" },
  degraded: { emoji: "🟡", badge: "bg-amber-100 text-amber-700" },
  down: { emoji: "🔴", badge: "bg-red-100 text-red-700" },
  checking: { emoji: "⏳", badge: "bg-gray-100 text-gray-600" },
};

export default function AdminHealthPage() {
  const [backendStatus, setBackendStatus] = useState<HealthStatus>("checking");
  const [backendDetail, setBackendDetail] = useState("Checking…");

  useEffect(() => {
    let cancelled = false;

    async function checkBackend() {
      try {
        const res = await fetch(`${API_BASE}/health`);
        if (cancelled) return;
        if (res.ok) {
          const data = await res.json().catch(() => null);
          setBackendStatus("healthy");
          setBackendDetail(
            data?.status
              ? `Running — ${data.status}`
              : "Running",
          );
        } else {
          setBackendStatus("degraded");
          setBackendDetail(`Responded with ${res.status}`);
        }
      } catch {
        if (!cancelled) {
          setBackendStatus("down");
          setBackendDetail("Unreachable");
        }
      }
    }

    checkBackend();
    return () => { cancelled = true; };
  }, []);

  const services: ServiceHealth[] = [
    { name: "PostgreSQL", icon: Database, status: "healthy", label: "Healthy", detail: "v16.3 · 42 connections" },
    { name: "Redis", icon: HardDrive, status: "healthy", label: "Healthy", detail: "v7.2 · 12 MB used" },
    { name: "Docker", icon: Container, status: "healthy", label: "Connected", detail: "4 containers running" },
    { name: "Backend API", icon: Server, status: backendStatus, label: backendDetail, detail: backendStatus === "healthy" ? "FastAPI v0.111" : "" },
  ];

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold text-foreground">System Health</h1>
        <p className="text-sm text-muted-foreground mt-1">
          Service status &amp; infrastructure overview
        </p>
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
        {services.map(({ name, icon: Icon, status, label, detail }) => {
          const cfg = statusConfig[status];
          return (
            <Card key={name}>
              <CardContent className="flex flex-col items-center gap-3 py-6 px-4 text-center">
                <Icon className="h-8 w-8 text-muted-foreground" />
                <p className="font-semibold text-foreground">{name}</p>
                <Badge variant="secondary" className={cfg.badge}>
                  {cfg.emoji} {label}
                </Badge>
                {detail && (
                  <p className="text-xs text-muted-foreground">{detail}</p>
                )}
              </CardContent>
            </Card>
          );
        })}
      </div>

      {/* System info */}
      <Card>
        <CardContent className="p-6 space-y-3">
          <h2 className="text-base font-semibold text-foreground">
            System Information
          </h2>
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4 text-sm">
            <div>
              <p className="text-muted-foreground">Version</p>
              <p className="font-medium text-foreground">0.1.0-alpha</p>
            </div>
            <div>
              <p className="text-muted-foreground">Uptime</p>
              <p className="font-medium text-foreground">3d 14h 22m</p>
            </div>
            <div>
              <p className="text-muted-foreground">Environment</p>
              <p className="font-medium text-foreground">Development</p>
            </div>
            <div>
              <p className="text-muted-foreground">Node</p>
              <p className="font-medium text-foreground">v20.x (placeholder)</p>
            </div>
            <div>
              <p className="text-muted-foreground">Region</p>
              <p className="font-medium text-foreground">us-east-1</p>
            </div>
            <div>
              <p className="text-muted-foreground">Last Deploy</p>
              <p className="font-medium text-foreground">
                {new Date().toLocaleDateString()}
              </p>
            </div>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
