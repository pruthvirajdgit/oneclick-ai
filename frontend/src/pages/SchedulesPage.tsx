import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { api } from "@/lib/api";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { CalendarClock, Clock, Loader2, Plus, Trash2 } from "lucide-react";

// ── Types ──────────────────────────────────────────────────────
interface Schedule {
  id: string;
  cron_expr: string;
  task_message: string;
  next_run_at: string | null;
  last_run_at: string | null;
  status: "active" | "paused";
}

interface Agent {
  id: string;
  status: string;
  model: string;
}

// ── Helpers ────────────────────────────────────────────────────
function relativeTime(iso: string | null): string {
  if (!iso) return "—";
  const diff = Date.now() - new Date(iso).getTime();
  if (diff < 0) {
    const absDiff = Math.abs(diff);
    const mins = Math.floor(absDiff / 60_000);
    if (mins < 60) return `in ${mins}m`;
    const hrs = Math.floor(mins / 60);
    if (hrs < 24) return `in ${hrs}h`;
    const days = Math.floor(hrs / 24);
    return `in ${days}d`;
  }
  const seconds = Math.floor(diff / 1000);
  if (seconds < 60) return "Just now";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

const CRON_DESCRIPTIONS: Record<string, string> = {
  "* * * * *": "Every minute",
  "*/5 * * * *": "Every 5 minutes",
  "*/15 * * * *": "Every 15 minutes",
  "*/30 * * * *": "Every 30 minutes",
  "0 * * * *": "Every hour",
  "0 */2 * * *": "Every 2 hours",
  "0 */3 * * *": "Every 3 hours",
  "0 */6 * * *": "Every 6 hours",
  "0 */12 * * *": "Every 12 hours",
  "0 0 * * *": "Daily at midnight",
  "0 9 * * *": "Daily at 9 AM",
  "0 0 * * MON": "Every Monday at midnight",
  "0 8 1 * *": "Monthly on the 1st at 8 AM",
};

function describeCron(expr: string): string {
  return CRON_DESCRIPTIONS[expr] ?? expr;
}

// ── Component ──────────────────────────────────────────────────
export default function SchedulesPage() {
  const [schedules, setSchedules] = useState<Schedule[]>([]);
  const [agents, setAgents] = useState<Agent[]>([]);
  const [loading, setLoading] = useState(true);
  const [createOpen, setCreateOpen] = useState(false);
  const [creating, setCreating] = useState(false);
  const [deletingId, setDeletingId] = useState<string | null>(null);

  // Form state
  const [agentId, setAgentId] = useState("");
  const [cronExpr, setCronExpr] = useState("");
  const [taskMessage, setTaskMessage] = useState("");

  const fetchSchedules = useCallback(async () => {
    try {
      const data = await api.get<Schedule[]>("/schedules");
      setSchedules(data);
    } catch {
      toast.error("Failed to load schedules");
    } finally {
      setLoading(false);
    }
  }, []);

  const fetchAgents = useCallback(async () => {
    try {
      const data = await api.get<Agent[]>("/agents");
      setAgents(data);
    } catch {
      // silently fail – agents are only needed for create
    }
  }, []);

  useEffect(() => {
    fetchSchedules();
    fetchAgents();
  }, [fetchSchedules, fetchAgents]);

  async function handleCreate() {
    if (!agentId || !cronExpr || !taskMessage) {
      toast.error("All fields are required");
      return;
    }
    setCreating(true);
    try {
      await api.post("/schedules", {
        agent_id: agentId,
        cron_expr: cronExpr,
        task_message: taskMessage,
      });
      toast.success("Schedule created!");
      setCreateOpen(false);
      setAgentId("");
      setCronExpr("");
      setTaskMessage("");
      await fetchSchedules();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : "Failed to create schedule");
    } finally {
      setCreating(false);
    }
  }

  async function handleDelete(id: string) {
    setDeletingId(id);
    try {
      await api.delete(`/schedules/${id}`);
      toast.success("Schedule deleted");
      await fetchSchedules();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : "Failed to delete schedule");
    } finally {
      setDeletingId(null);
    }
  }

  if (loading) {
    return (
      <div className="space-y-6">
        <div className="flex items-center justify-between">
          <div className="h-8 w-48 animate-pulse rounded-lg bg-muted" />
          <div className="h-8 w-36 animate-pulse rounded-lg bg-muted" />
        </div>
        <Card className="shadow-sm">
          <CardContent className="space-y-3 py-4">
            {Array.from({ length: 3 }).map((_, i) => (
              <div key={i} className="h-12 w-full animate-pulse rounded bg-muted" />
            ))}
          </CardContent>
        </Card>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold text-foreground">Scheduled Jobs</h1>
          <p className="text-muted-foreground">Manage your automated tasks</p>
        </div>
        <Button
          className="bg-indigo-600 text-white hover:bg-indigo-700"
          onClick={() => setCreateOpen(true)}
        >
          <Plus className="mr-1.5 h-4 w-4" />
          New Schedule
        </Button>
      </div>

      {/* Empty state */}
      {schedules.length === 0 && (
        <Card className="flex flex-col items-center justify-center py-16 shadow-sm">
          <CalendarClock className="mb-4 h-12 w-12 text-muted-foreground/50" />
          <p className="mb-1 text-lg font-medium text-foreground">No schedules yet</p>
          <p className="mb-6 text-sm text-muted-foreground">
            Create a scheduled job to automate your tasks.
          </p>
          <Button
            className="bg-indigo-600 text-white hover:bg-indigo-700"
            onClick={() => setCreateOpen(true)}
          >
            <Plus className="mr-1.5 h-4 w-4" />
            New Schedule
          </Button>
        </Card>
      )}

      {/* Schedules table */}
      {schedules.length > 0 && (
        <Card className="shadow-sm">
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <CalendarClock className="h-5 w-5 text-indigo-500" />
              Schedules
            </CardTitle>
          </CardHeader>
          <CardContent>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Cron</TableHead>
                  <TableHead>Task</TableHead>
                  <TableHead>Next Run</TableHead>
                  <TableHead>Last Run</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead className="w-[60px]" />
                </TableRow>
              </TableHeader>
              <TableBody>
                {schedules.map((s) => (
                  <TableRow key={s.id}>
                    <TableCell>
                      <div>
                        <p className="font-mono text-sm">{s.cron_expr}</p>
                        <p className="text-xs text-muted-foreground">
                          {describeCron(s.cron_expr)}
                        </p>
                      </div>
                    </TableCell>
                    <TableCell className="max-w-[250px] truncate">
                      {s.task_message}
                    </TableCell>
                    <TableCell>
                      <div className="flex items-center gap-1 text-sm text-muted-foreground">
                        <Clock className="h-3.5 w-3.5" />
                        {relativeTime(s.next_run_at)}
                      </div>
                    </TableCell>
                    <TableCell className="text-sm text-muted-foreground">
                      {relativeTime(s.last_run_at)}
                    </TableCell>
                    <TableCell>
                      <Badge
                        variant={s.status === "active" ? "default" : "secondary"}
                      >
                        {s.status}
                      </Badge>
                    </TableCell>
                    <TableCell>
                      <Button
                        variant="ghost"
                        size="sm"
                        className="text-destructive hover:text-destructive"
                        onClick={() => handleDelete(s.id)}
                        disabled={deletingId === s.id}
                      >
                        {deletingId === s.id ? (
                          <Loader2 className="h-4 w-4 animate-spin" />
                        ) : (
                          <Trash2 className="h-4 w-4" />
                        )}
                      </Button>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      )}

      {/* ── Create Schedule Dialog ──────────────────────────── */}
      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>New Schedule</DialogTitle>
            <DialogDescription>
              Set up an automated recurring task for your agent.
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-4 py-2">
            <div className="space-y-1.5">
              <Label htmlFor="agent-select">Agent</Label>
              <Select value={agentId} onValueChange={(v) => setAgentId(v ?? "")}>
                <SelectTrigger id="agent-select">
                  <SelectValue placeholder="Select an agent" />
                </SelectTrigger>
                <SelectContent>
                  {agents.map((a) => (
                    <SelectItem key={a.id} value={a.id}>
                      {a.id} ({a.model})
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <div className="space-y-1.5">
              <Label htmlFor="cron-input">Cron Expression</Label>
              <Input
                id="cron-input"
                value={cronExpr}
                onChange={(e) => setCronExpr(e.target.value)}
                placeholder="0 */3 * * *"
              />
              <p className="text-xs text-muted-foreground">
                e.g., <code className="rounded bg-muted px-1">0 */3 * * *</code> for every 3 hours
              </p>
            </div>

            <div className="space-y-1.5">
              <Label htmlFor="task-message">Task Message</Label>
              <Textarea
                id="task-message"
                value={taskMessage}
                onChange={(e) => setTaskMessage(e.target.value)}
                placeholder="Check flight prices from SFO to NYC"
                rows={3}
              />
            </div>
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={() => setCreateOpen(false)}>
              Cancel
            </Button>
            <Button
              className="bg-indigo-600 text-white hover:bg-indigo-700"
              onClick={handleCreate}
              disabled={creating}
            >
              {creating && <Loader2 className="mr-1.5 h-4 w-4 animate-spin" />}
              {creating ? "Creating…" : "Create"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
