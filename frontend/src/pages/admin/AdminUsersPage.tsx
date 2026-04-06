import { useMemo, useState } from "react";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Search } from "lucide-react";

interface MockUser {
  id: string;
  email: string;
  tier: "free" | "pro";
  agentCount: number;
  totalRequests: number;
  joinedDate: string;
}

const MOCK_USERS: MockUser[] = [
  { id: "u1", email: "alice@example.com", tier: "pro", agentCount: 5, totalRequests: 12430, joinedDate: "2024-11-02" },
  { id: "u2", email: "bob@startup.io", tier: "free", agentCount: 1, totalRequests: 342, joinedDate: "2025-01-15" },
  { id: "u3", email: "carol@acme.dev", tier: "pro", agentCount: 8, totalRequests: 45200, joinedDate: "2024-09-20" },
  { id: "u4", email: "dave@bigcorp.com", tier: "free", agentCount: 2, totalRequests: 1100, joinedDate: "2025-02-01" },
  { id: "u5", email: "eve@research.org", tier: "pro", agentCount: 3, totalRequests: 8750, joinedDate: "2024-12-10" },
  { id: "u6", email: "frank@dev.team", tier: "free", agentCount: 1, totalRequests: 78, joinedDate: "2025-05-22" },
  { id: "u7", email: "grace@ml-lab.ai", tier: "pro", agentCount: 12, totalRequests: 98340, joinedDate: "2024-08-05" },
  { id: "u8", email: "hank@freelance.me", tier: "free", agentCount: 0, totalRequests: 15, joinedDate: "2025-06-01" },
];

export default function AdminUsersPage() {
  const [search, setSearch] = useState("");

  const filtered = useMemo(() => {
    if (!search.trim()) return MOCK_USERS;
    const q = search.toLowerCase();
    return MOCK_USERS.filter(
      (u) =>
        u.email.toLowerCase().includes(q) ||
        u.tier.includes(q),
    );
  }, [search]);

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold text-foreground">Users</h1>
        <p className="text-sm text-muted-foreground mt-1">
          Manage all registered users
        </p>
      </div>

      <div className="relative max-w-sm">
        <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
        <Input
          placeholder="Search by email or tier…"
          className="pl-9"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>

      <div className="rounded-lg border border-border bg-card overflow-hidden">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Email</TableHead>
              <TableHead>Tier</TableHead>
              <TableHead className="text-right">Agents</TableHead>
              <TableHead className="text-right">Requests</TableHead>
              <TableHead>Joined</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {filtered.length === 0 ? (
              <TableRow>
                <TableCell colSpan={5} className="text-center py-8 text-muted-foreground">
                  No users match your search.
                </TableCell>
              </TableRow>
            ) : (
              filtered.map((user, idx) => (
                <TableRow
                  key={user.id}
                  className={idx % 2 === 1 ? "bg-muted/40" : ""}
                >
                  <TableCell className="font-medium">{user.email}</TableCell>
                  <TableCell>
                    <Badge
                      variant={user.tier === "pro" ? "default" : "secondary"}
                      className={
                        user.tier === "pro"
                          ? "bg-[#8b5cf6] hover:bg-[#7c3aed] text-white"
                          : ""
                      }
                    >
                      {user.tier}
                    </Badge>
                  </TableCell>
                  <TableCell className="text-right">{user.agentCount}</TableCell>
                  <TableCell className="text-right">
                    {user.totalRequests.toLocaleString()}
                  </TableCell>
                  <TableCell className="text-muted-foreground">
                    {new Date(user.joinedDate).toLocaleDateString()}
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
