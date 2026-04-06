import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Bell, CheckCircle2, AlertTriangle, Info } from "lucide-react";

const notifications = [
  {
    id: 1,
    title: "Deployment complete",
    message: "Your model pipeline was deployed successfully.",
    type: "success" as const,
    time: "2 min ago",
    read: false,
  },
  {
    id: 2,
    title: "Usage alert",
    message: "You've reached 80% of your monthly API quota.",
    type: "warning" as const,
    time: "1 hour ago",
    read: false,
  },
  {
    id: 3,
    title: "System update",
    message: "OneClick.ai v2.1 is now available with new features.",
    type: "info" as const,
    time: "3 hours ago",
    read: true,
  },
];

const iconMap = {
  success: CheckCircle2,
  warning: AlertTriangle,
  info: Info,
};

const colorMap = {
  success: "text-emerald-500",
  warning: "text-amber-500",
  info: "text-blue-500",
};

export default function NotificationsPage() {
  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold text-foreground">
            Notifications
          </h1>
          <p className="text-muted-foreground">
            Stay up to date with your account activity
          </p>
        </div>
        <Badge variant="secondary" className="gap-1">
          <Bell className="h-3 w-3" />
          {notifications.filter((n) => !n.read).length} unread
        </Badge>
      </div>

      <div className="space-y-3">
        {notifications.map((n) => {
          const Icon = iconMap[n.type];
          return (
            <Card
              key={n.id}
              className={`shadow-sm transition-colors ${!n.read ? "border-primary/20 bg-primary/[0.02]" : ""}`}
            >
              <CardHeader className="flex flex-row items-start gap-3 py-3">
                <Icon className={`h-5 w-5 mt-0.5 ${colorMap[n.type]}`} />
                <div className="flex-1">
                  <div className="flex items-center justify-between">
                    <CardTitle className="text-base">{n.title}</CardTitle>
                    <span className="text-xs text-muted-foreground">
                      {n.time}
                    </span>
                  </div>
                  <CardContent className="p-0 pt-1">
                    <p className="text-sm text-muted-foreground">{n.message}</p>
                  </CardContent>
                </div>
              </CardHeader>
            </Card>
          );
        })}
      </div>
    </div>
  );
}
