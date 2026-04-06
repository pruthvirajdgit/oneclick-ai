import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { BarChart3, CalendarClock, MessageSquare, Zap } from "lucide-react";

const stats = [
  { label: "Active Chats", value: "3", icon: MessageSquare, color: "text-primary" },
  { label: "Scheduled Jobs", value: "12", icon: CalendarClock, color: "text-emerald-500" },
  { label: "API Calls Today", value: "1,248", icon: Zap, color: "text-amber-500" },
  { label: "Total Usage", value: "$24.50", icon: BarChart3, color: "text-violet-500" },
];

export default function DashboardPage() {
  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-semibold text-foreground">Dashboard</h1>
        <p className="text-muted-foreground">
          Welcome back! Here&apos;s an overview of your account.
        </p>
      </div>

      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        {stats.map(({ label, value, icon: Icon, color }) => (
          <Card key={label} className="shadow-sm">
            <CardHeader className="flex flex-row items-center justify-between pb-2">
              <CardTitle className="text-sm font-medium text-muted-foreground">
                {label}
              </CardTitle>
              <Icon className={`h-4 w-4 ${color}`} />
            </CardHeader>
            <CardContent>
              <div className="text-2xl font-bold">{value}</div>
            </CardContent>
          </Card>
        ))}
      </div>

      <Card className="shadow-sm">
        <CardHeader>
          <CardTitle>Recent Activity</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-4">
            {[
              { action: "Chat created", detail: "Marketing copy generation", status: "running" },
              { action: "Schedule completed", detail: "Daily report", status: "completed" },
              { action: "Error detected", detail: "Image pipeline timeout", status: "error" },
            ].map((item, i) => (
              <div
                key={i}
                className="flex items-center justify-between rounded-lg border border-border p-3"
              >
                <div>
                  <p className="text-sm font-medium">{item.action}</p>
                  <p className="text-xs text-muted-foreground">{item.detail}</p>
                </div>
                <Badge
                  variant={
                    item.status === "running"
                      ? "default"
                      : item.status === "error"
                        ? "destructive"
                        : "secondary"
                  }
                >
                  {item.status}
                </Badge>
              </div>
            ))}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
