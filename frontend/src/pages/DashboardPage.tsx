import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { api } from "@/lib/api";
import {
  Card,
  CardContent,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Bot,
  Clock,
  Loader2,
  MessageSquare,
  Plus,
  Trash2,
} from "lucide-react";

// ── Types ──────────────────────────────────────────────────────
interface Agent {
  id: string;
  status: "running" | "stopped" | "error" | "creating";
  model: string;
  last_active: string | null;
  created_at: string;
  chat_url: string | null;
}

// ── Helpers ────────────────────────────────────────────────────
function relativeTime(iso: string | null): string {
  if (!iso) return "Never";
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

const STATUS_CONFIG: Record<
  Agent["status"],
  { dot: string; label: string; variant: "default" | "secondary" | "destructive" | "outline" }
> = {
  running: { dot: "bg-emerald-500", label: "Running", variant: "outline" },
  stopped: { dot: "bg-gray-400", label: "Stopped", variant: "secondary" },
  error: { dot: "bg-red-500", label: "Error", variant: "destructive" },
  creating: { dot: "bg-amber-400", label: "Creating", variant: "outline" },
};

const REFRESH_INTERVAL = 30_000;

// ── Component ──────────────────────────────────────────────────
export default function DashboardPage() {
  const [agents, setAgents] = useState<Agent[]>([]);
  const [loading, setLoading] = useState(true);
  const [createOpen, setCreateOpen] = useState(false);
  const [creating, setCreating] = useState(false);
  const [model, setModel] = useState("groq/llama-3.3-70b-versatile");
  const [deleteTarget, setDeleteTarget] = useState<Agent | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [wakingId, setWakingId] = useState<string | null>(null);

  // ── Fetch agents ───────────────────────────────────────────
  const fetchAgents = useCallback(async (showLoading = false) => {
    if (showLoading) setLoading(true);
    try {
      const data = await api.get<Agent[]>("/agents");
      setAgents(data);
    } catch {
      if (showLoading) toast.error("Failed to load agents");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchAgents(true);
  }, [fetchAgents]);

  // Auto-refresh every 30s
  const fetchRef = useRef(fetchAgents);
  fetchRef.current = fetchAgents;
  useEffect(() => {
    const id = setInterval(() => fetchRef.current(false), REFRESH_INTERVAL);
    return () => clearInterval(id);
  }, []);

  // ── Create agent ───────────────────────────────────────────
  async function handleCreate() {
    setCreating(true);
    try {
      await api.post("/agents", { model });
      toast.success("Agent created!");
      setCreateOpen(false);
      setModel("groq/llama-3.3-70b-versatile");
      await fetchAgents(false);
    } catch (err) {
      toast.error(err instanceof Error ? err.message : "Failed to create agent");
    } finally {
      setCreating(false);
    }
  }

  // ── Delete agent ───────────────────────────────────────────
  async function handleDelete() {
    if (!deleteTarget) return;
    setDeleting(true);
    try {
      await api.delete(`/agents/${deleteTarget.id}`);
      toast.success("Agent deleted");
      setDeleteTarget(null);
      await fetchAgents(false);
    } catch (err) {
      toast.error(err instanceof Error ? err.message : "Failed to delete agent");
    } finally {
      setDeleting(false);
    }
  }

  // ── Wake agent & open OpenClaw UI in new tab ──────────────
  async function handleChat(agent: Agent) {
    setWakingId(agent.id);
    try {
      const result = await api.post<{ status: string; chat_url: string }>(
        `/agents/${agent.id}/wake`,
        {}
      );
      window.open(result.chat_url, "_blank", "noopener");
      await fetchAgents(false);
    } catch (err) {
      toast.error(
        err instanceof Error ? err.message : "Failed to wake agent"
      );
    } finally {
      setWakingId(null);
    }
  }

  // ── Loading skeleton ───────────────────────────────────────
  if (loading) {
    return (
      <div className="space-y-6">
        <div className="flex items-center justify-between">
          <div className="h-8 w-40 animate-pulse rounded-lg bg-muted" />
          <div className="h-8 w-32 animate-pulse rounded-lg bg-muted" />
        </div>
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {Array.from({ length: 3 }).map((_, i) => (
            <Card key={i} className="shadow-sm">
              <CardHeader>
                <div className="h-5 w-3/4 animate-pulse rounded bg-muted" />
              </CardHeader>
              <CardContent className="space-y-3">
                <div className="h-4 w-1/2 animate-pulse rounded bg-muted" />
                <div className="h-4 w-2/3 animate-pulse rounded bg-muted" />
              </CardContent>
            </Card>
          ))}
        </div>
      </div>
    );
  }

  // ── Render ─────────────────────────────────────────────────
  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold text-foreground">Your Agents</h1>
        <Button
          className="bg-indigo-600 text-white hover:bg-indigo-700"
          onClick={() => setCreateOpen(true)}
        >
          <Plus className="mr-1.5 h-4 w-4" />
          Create Agent
        </Button>
      </div>

      {/* Empty state */}
      {agents.length === 0 && (
        <Card className="flex flex-col items-center justify-center py-16 shadow-sm">
          <Bot className="mb-4 h-12 w-12 text-muted-foreground/50" />
          <p className="mb-1 text-lg font-medium text-foreground">
            No agents yet
          </p>
          <p className="mb-6 text-sm text-muted-foreground">
            Create your first AI agent to get started!
          </p>
          <Button
            className="bg-indigo-600 text-white hover:bg-indigo-700"
            size="lg"
            onClick={() => setCreateOpen(true)}
          >
            <Plus className="mr-1.5 h-4 w-4" />
            Create Agent
          </Button>
        </Card>
      )}

      {/* Agent grid */}
      {agents.length > 0 && (
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {agents.map((agent) => {
            const status = STATUS_CONFIG[agent.status] ?? STATUS_CONFIG.stopped;
            return (
              <Card
                key={agent.id}
                className="shadow-sm transition-shadow hover:shadow-md"
              >
                <CardHeader>
                  <div className="flex items-start justify-between gap-2">
                    <CardTitle
                      className="truncate text-base"
                      title={agent.id}
                    >
                      {agent.id}
                    </CardTitle>
                    <Badge variant={status.variant} className="shrink-0">
                      <span
                        className={`mr-1.5 inline-block h-2 w-2 rounded-full ${status.dot}`}
                      />
                      {status.label}
                    </Badge>
                  </div>
                </CardHeader>

                <CardContent className="space-y-2 text-sm text-muted-foreground">
                  <div className="flex items-center gap-1.5">
                    <Bot className="h-3.5 w-3.5" />
                    <span className="truncate">{agent.model}</span>
                  </div>
                  <div className="flex items-center gap-1.5">
                    <Clock className="h-3.5 w-3.5" />
                    <span>Active {relativeTime(agent.last_active)}</span>
                  </div>
                  <div className="text-xs">
                    Created{" "}
                    {new Date(agent.created_at).toLocaleDateString(undefined, {
                      month: "short",
                      day: "numeric",
                      year: "numeric",
                    })}
                  </div>
                </CardContent>

                <CardFooter className="gap-2">
                  <Button
                    size="sm"
                    className="flex-1 bg-indigo-600 text-white hover:bg-indigo-700"
                    onClick={() => handleChat(agent)}
                    disabled={wakingId === agent.id}
                  >
                    {wakingId === agent.id ? (
                      <>
                        <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />
                        Waking…
                      </>
                    ) : (
                      <>
                        <MessageSquare className="mr-1.5 h-3.5 w-3.5" />
                        Chat
                      </>
                    )}
                  </Button>
                  <Button
                    size="sm"
                    variant="destructive"
                    onClick={() => setDeleteTarget(agent)}
                  >
                    <Trash2 className="mr-1 h-3.5 w-3.5" />
                    Delete
                  </Button>
                </CardFooter>
              </Card>
            );
          })}
        </div>
      )}

      {/* ── Create Agent Dialog ─────────────────────────────── */}
      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Create Agent</DialogTitle>
            <DialogDescription>
              Choose a model for your new AI agent.
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-3 py-2">
            <div className="space-y-1.5">
              <Label htmlFor="model-select">Model</Label>
              <select
                id="model-select"
                value={model}
                onChange={(e) => setModel(e.target.value)}
                className="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              >
                <optgroup label="Groq (Fast, Free Tier)">
                  <option value="groq/llama-3.3-70b-versatile">Llama 3.3 70B — Best quality</option>
                  <option value="groq/llama-3.1-8b-instant">Llama 3.1 8B — Fast responses</option>
                  <option value="groq/llama-guard-3-8b">Llama Guard 3 8B — Safety-focused</option>
                  <option value="groq/gemma2-9b-it">Gemma 2 9B — Google model</option>
                  <option value="groq/mixtral-8x7b-32768">Mixtral 8x7B — Large context</option>
                </optgroup>
                <optgroup label="OpenRouter (Multi-provider)">
                  <option value="openrouter/auto">Auto — Best available</option>
                  <option value="openrouter/nvidia/llama-3.1-nemotron-70b-instruct:free">Nemotron 70B — Free</option>
                  <option value="openrouter/meta-llama/llama-3.3-70b-instruct:free">Llama 3.3 70B — Free</option>
                  <option value="openrouter/qwen/qwen-2.5-72b-instruct:free">Qwen 2.5 72B — Free</option>
                  <option value="openrouter/google/gemma-2-9b-it:free">Gemma 2 9B — Free</option>
                </optgroup>
              </select>
              <p className="text-xs text-muted-foreground">
                Groq models are fastest. OpenRouter provides free fallback options.
              </p>
            </div>
          </div>

          <DialogFooter>
            <Button
              className="bg-indigo-600 text-white hover:bg-indigo-700"
              onClick={handleCreate}
              disabled={creating}
            >
              {creating && (
                <Loader2 className="mr-1.5 h-4 w-4 animate-spin" />
              )}
              {creating ? "Creating…" : "Create"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* ── Delete Confirmation Dialog ──────────────────────── */}
      <Dialog
        open={deleteTarget !== null}
        onOpenChange={(open) => {
          if (!open) setDeleteTarget(null);
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete Agent</DialogTitle>
            <DialogDescription>
              Are you sure? This will permanently delete the agent and all its
              data.
            </DialogDescription>
          </DialogHeader>

          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setDeleteTarget(null)}
              disabled={deleting}
            >
              Cancel
            </Button>
            <Button
              variant="destructive"
              onClick={handleDelete}
              disabled={deleting}
            >
              {deleting && (
                <Loader2 className="mr-1.5 h-4 w-4 animate-spin" />
              )}
              {deleting ? "Deleting…" : "Delete"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
