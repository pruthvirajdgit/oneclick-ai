import { useMemo, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

type AgentStatus = "running" | "stopped" | "error";

interface MockAgent {
  id: string;
  ownerEmail: string;
  model: string;
  status: AgentStatus;
  memoryMb: number;
  lastActive: string;
  created: string;
}

const MOCK_AGENTS: MockAgent[] = [
  { id: "agt-001", ownerEmail: "alice@example.com", model: "gpt-4o", status: "running", memoryMb: 256, lastActive: "2025-06-20T14:30:00Z", created: "2025-01-10" },
  { id: "agt-002", ownerEmail: "alice@example.com", model: "gpt-4o-mini", status: "running", memoryMb: 128, lastActive: "2025-06-20T14:25:00Z", created: "2025-02-15" },
  { id: "agt-003", ownerEmail: "bob@startup.io", model: "gpt-4o-mini", status: "stopped", memoryMb: 64, lastActive: "2025-06-18T09:00:00Z", created: "2025-03-01" },
  { id: "agt-004", ownerEmail: "carol@acme.dev", model: "gpt-4o", status: "running", memoryMb: 512, lastActive: "2025-06-20T14:28:00Z", created: "2024-12-05" },
  { id: "agt-005", ownerEmail: "carol@acme.dev", model: "gpt-4o", status: "error", memoryMb: 256, lastActive: "2025-06-19T22:10:00Z", created: "2025-01-20" },
  { id: "agt-006", ownerEmail: "carol@acme.dev", model: "gpt-4o-mini", status: "running", memoryMb: 128, lastActive: "2025-06-20T14:15:00Z", created: "2025-04-10" },
  { id: "agt-007", ownerEmail: "dave@bigcorp.com", model: "gpt-4o-mini", status: "stopped", memoryMb: 64, lastActive: "2025-06-15T11:00:00Z", created: "2025-05-01" },
  { id: "agt-008", ownerEmail: "eve@research.org", model: "gpt-4o", status: "running", memoryMb: 512, lastActive: "2025-06-20T14:00:00Z", created: "2025-01-02" },
  { id: "agt-009", ownerEmail: "grace@ml-lab.ai", model: "gpt-4o", status: "running", memoryMb: 1024, lastActive: "2025-06-20T14:29:00Z", created: "2024-09-15" },
  { id: "agt-010", ownerEmail: "grace@ml-lab.ai", model: "gpt-4o-mini", status: "error", memoryMb: 128, lastActive: "2025-06-20T08:45:00Z", created: "2025-03-20" },
];

const STATUS_FILTERS = ["all", "running", "stopped", "error"] as const;

const statusColor: Record<AgentStatus, string> = {
  running: "bg-emerald-100 text-emerald-700",
  stopped: "bg-gray-100 text-gray-600",
  error: "bg-red-100 text-red-700",
};

export default function AdminAgentsPage() {
  const [filter, setFilter] = useState<(typeof STATUS_FILTERS)[number]>("all");

  const filtered = useMemo(() => {
    if (filter === "all") return MOCK_AGENTS;
    return MOCK_AGENTS.filter((a) => a.status === filter);
  }, [filter]);

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold text-foreground">Agents</h1>
        <p className="text-sm text-muted-foreground mt-1">
          All agents across every user
        </p>
      </div>

      <div className="flex gap-2">
        {STATUS_FILTERS.map((s) => (
          <Button
            key={s}
            variant={filter === s ? "default" : "outline"}
            size="sm"
            className={
              filter === s
                ? "bg-[#8b5cf6] hover:bg-[#7c3aed] text-white"
                : ""
            }
            onClick={() => setFilter(s)}
          >
            {s.charAt(0).toUpperCase() + s.slice(1)}
          </Button>
        ))}
      </div>

      <div className="rounded-lg border border-border bg-card overflow-hidden">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Agent ID</TableHead>
              <TableHead>Owner</TableHead>
              <TableHead>Model</TableHead>
              <TableHead>Status</TableHead>
              <TableHead className="text-right">Memory</TableHead>
              <TableHead>Last Active</TableHead>
              <TableHead>Created</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {filtered.length === 0 ? (
              <TableRow>
                <TableCell colSpan={7} className="text-center py-8 text-muted-foreground">
                  No agents match the selected filter.
                </TableCell>
              </TableRow>
            ) : (
              filtered.map((agent, idx) => (
                <TableRow
                  key={agent.id}
                  className={idx % 2 === 1 ? "bg-muted/40" : ""}
                >
                  <TableCell className="font-mono text-sm">{agent.id}</TableCell>
                  <TableCell>{agent.ownerEmail}</TableCell>
                  <TableCell className="font-mono text-sm">{agent.model}</TableCell>
                  <TableCell>
                    <Badge
                      variant="secondary"
                      className={statusColor[agent.status]}
                    >
                      {agent.status}
                    </Badge>
                  </TableCell>
                  <TableCell className="text-right">{agent.memoryMb} MB</TableCell>
                  <TableCell className="text-muted-foreground">
                    {new Date(agent.lastActive).toLocaleString()}
                  </TableCell>
                  <TableCell className="text-muted-foreground">
                    {new Date(agent.created).toLocaleDateString()}
                  </TableCell>
                </TableRow>
              ))
            )}
          </TableBody>
        </Table>
      </div>
    </div>
  );
}
