import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { api } from "@/lib/api";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Bell, CheckCheck, Loader2 } from "lucide-react";

interface Notification {
  id: number;
  user_id: string;
  title: string;
  body: string;
  read: boolean;
  created_at: string;
}

function relativeTime(iso: string): string {
  const diff = Date.now() - new Date(iso).getTime();
  const seconds = Math.floor(diff / 1000);
  if (seconds < 60) return "Just now";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

export default function NotificationsPage() {
  const [notifications, setNotifications] = useState<Notification[]>([]);
  const [loading, setLoading] = useState(true);
  const [markingAll, setMarkingAll] = useState(false);
  const [markingId, setMarkingId] = useState<number | null>(null);

  const fetchNotifications = useCallback(async () => {
    try {
      const data = await api.get<Notification[]>("/notifications");
      setNotifications(data);
    } catch {
      toast.error("Failed to load notifications");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchNotifications();
  }, [fetchNotifications]);

  const unreadCount = notifications.filter((n) => !n.read).length;

  async function markAsRead(id: number) {
    setMarkingId(id);
    try {
      await api.post(`/notifications/${id}/read`, {});
      setNotifications((prev) =>
        prev.map((n) => (n.id === id ? { ...n, read: true } : n))
      );
    } catch {
      toast.error("Failed to mark as read");
    } finally {
      setMarkingId(null);
    }
  }

  async function markAllAsRead() {
    setMarkingAll(true);
    try {
      const unread = notifications.filter((n) => !n.read);
      await Promise.all(
        unread.map((n) => api.post(`/notifications/${n.id}/read`, {}))
      );
      setNotifications((prev) => prev.map((n) => ({ ...n, read: true })));
      toast.success("All notifications marked as read");
    } catch {
      toast.error("Failed to mark all as read");
    } finally {
      setMarkingAll(false);
    }
  }

  if (loading) {
    return (
      <div className="space-y-6">
        <div className="h-8 w-48 animate-pulse rounded-lg bg-muted" />
        <div className="space-y-3">
          {Array.from({ length: 4 }).map((_, i) => (
            <div key={i} className="h-20 w-full animate-pulse rounded-lg bg-muted" />
          ))}
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <div>
            <h1 className="text-2xl font-semibold text-foreground">Notifications</h1>
            <p className="text-muted-foreground">Stay up to date with your alerts</p>
          </div>
          {unreadCount > 0 && (
            <Badge className="bg-indigo-600 text-white">
              {unreadCount} unread
            </Badge>
          )}
        </div>
        {unreadCount > 0 && (
          <Button variant="outline" onClick={markAllAsRead} disabled={markingAll}>
            {markingAll ? (
              <Loader2 className="mr-1.5 h-4 w-4 animate-spin" />
            ) : (
              <CheckCheck className="mr-1.5 h-4 w-4" />
            )}
            Mark all as read
          </Button>
        )}
      </div>

      {/* Empty state */}
      {notifications.length === 0 && (
        <Card className="flex flex-col items-center justify-center py-16 shadow-sm">
          <Bell className="mb-4 h-12 w-12 text-muted-foreground/50" />
          <p className="mb-1 text-lg font-medium text-foreground">No notifications</p>
          <p className="text-sm text-muted-foreground">
            You&apos;re all caught up! Notifications will appear here.
          </p>
        </Card>
      )}

      {/* Notification list */}
      {notifications.length > 0 && (
        <div className="space-y-3">
          {notifications.map((n) => (
            <Card
              key={n.id}
              className={`shadow-sm transition-colors ${
                !n.read ? "border-indigo-200 bg-indigo-50/50" : ""
              }`}
            >
              <CardHeader className="flex flex-row items-start gap-3 py-3 px-4">
                {/* Unread dot */}
                <div className="mt-1.5 flex-shrink-0">
                  {!n.read ? (
                    <div className="h-2.5 w-2.5 rounded-full bg-indigo-500" />
                  ) : (
                    <div className="h-2.5 w-2.5" />
                  )}
                </div>

                <div className="flex-1 min-w-0">
                  <div className="flex items-center justify-between gap-2">
                    <CardTitle
                      className={`text-base ${!n.read ? "font-semibold" : "font-medium"}`}
                    >
                      {n.title}
                    </CardTitle>
                    <span className="shrink-0 text-xs text-muted-foreground">
                      {relativeTime(n.created_at)}
                    </span>
                  </div>
                  <CardContent className="p-0 pt-1">
                    <p className="text-sm text-muted-foreground">{n.body}</p>
                  </CardContent>
                </div>

                {!n.read && (
                  <Button
                    variant="ghost"
                    size="sm"
                    className="shrink-0 text-xs"
                    onClick={() => markAsRead(n.id)}
                    disabled={markingId === n.id}
                  >
                    {markingId === n.id ? (
                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    ) : (
                      "Mark read"
                    )}
                  </Button>
                )}
              </CardHeader>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}
