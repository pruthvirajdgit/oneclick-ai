import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { api } from "@/lib/api";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { BarChart3, ArrowDownToLine, ArrowUpFromLine, Zap } from "lucide-react";

interface DayUsage {
  requests: number;
  limit: number;
  tokens_in: number;
  tokens_out: number;
}

interface AllTimeUsage {
  requests: number;
  tokens_in: number;
  tokens_out: number;
}

interface UsageData {
  today: DayUsage;
  all_time: AllTimeUsage;
}

function formatNumber(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return n.toLocaleString();
}

function usageColor(pct: number): string {
  if (pct > 80) return "bg-red-500";
  if (pct >= 50) return "bg-yellow-500";
  return "bg-emerald-500";
}

export default function UsagePage() {
  const [usage, setUsage] = useState<UsageData | null>(null);
  const [loading, setLoading] = useState(true);

  const fetchUsage = useCallback(async () => {
    try {
      const data = await api.get<UsageData>("/usage");
      setUsage(data);
    } catch {
      toast.error("Failed to load usage data");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchUsage();
  }, [fetchUsage]);

  if (loading) {
    return (
      <div className="space-y-6">
        <div className="h-8 w-40 animate-pulse rounded-lg bg-muted" />
        <div className="grid gap-6 md:grid-cols-2">
          {Array.from({ length: 2 }).map((_, i) => (
            <Card key={i} className="shadow-sm">
              <CardHeader><div className="h-5 w-3/4 animate-pulse rounded bg-muted" /></CardHeader>
              <CardContent className="space-y-3">
                <div className="h-4 w-full animate-pulse rounded bg-muted" />
                <div className="h-4 w-2/3 animate-pulse rounded bg-muted" />
              </CardContent>
            </Card>
          ))}
        </div>
      </div>
    );
  }

  const today = usage?.today ?? { requests: 0, limit: 50, tokens_in: 0, tokens_out: 0 };
  const allTime = usage?.all_time ?? { requests: 0, tokens_in: 0, tokens_out: 0 };
  const pct = today.limit > 0 ? (today.requests / today.limit) * 100 : 0;

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold text-foreground">Usage</h1>
          <p className="text-muted-foreground">Monitor your API usage</p>
        </div>
        <Badge variant="outline" className="gap-1.5 text-sm">
          <Zap className="h-3.5 w-3.5 text-indigo-500" />
          Free Tier — {today.limit} requests/day
        </Badge>
      </div>

      {/* Cards grid */}
      <div className="grid gap-6 md:grid-cols-2">
        {/* Today's Usage */}
        <Card className="shadow-sm">
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-lg">
              <BarChart3 className="h-5 w-5 text-indigo-500" />
              Today's Usage
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-5">
            {/* Progress bar */}
            <div className="space-y-2">
              <div className="flex items-center justify-between text-sm">
                <span className="text-muted-foreground">Requests</span>
                <span className="font-medium">
                  {today.requests} / {today.limit}
                </span>
              </div>
              <div className="h-3 w-full overflow-hidden rounded-full bg-muted">
                <div
                  className={`h-full rounded-full transition-all ${usageColor(pct)}`}
                  style={{ width: `${Math.min(pct, 100)}%` }}
                />
              </div>
              <p className="text-xs text-muted-foreground">
                {Math.round(pct)}% of daily limit used
              </p>
            </div>

            {/* Token stats */}
            <div className="grid grid-cols-2 gap-4">
              <div className="flex items-center gap-2 rounded-lg border p-3">
                <ArrowDownToLine className="h-4 w-4 text-indigo-500" />
                <div>
                  <p className="text-xs text-muted-foreground">Tokens In</p>
                  <p className="text-lg font-semibold">{formatNumber(today.tokens_in)}</p>
                </div>
              </div>
              <div className="flex items-center gap-2 rounded-lg border p-3">
                <ArrowUpFromLine className="h-4 w-4 text-indigo-500" />
                <div>
                  <p className="text-xs text-muted-foreground">Tokens Out</p>
                  <p className="text-lg font-semibold">{formatNumber(today.tokens_out)}</p>
                </div>
              </div>
            </div>
          </CardContent>
        </Card>

        {/* All-Time Stats */}
        <Card className="shadow-sm">
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-lg">
              <BarChart3 className="h-5 w-5 text-indigo-500" />
              All-Time Stats
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="rounded-lg border p-4 text-center">
              <p className="text-xs text-muted-foreground">Total Requests</p>
              <p className="text-3xl font-bold">{formatNumber(allTime.requests)}</p>
            </div>
            <div className="grid grid-cols-2 gap-4">
              <div className="flex items-center gap-2 rounded-lg border p-3">
                <ArrowDownToLine className="h-4 w-4 text-indigo-500" />
                <div>
                  <p className="text-xs text-muted-foreground">Total Tokens In</p>
                  <p className="text-lg font-semibold">{formatNumber(allTime.tokens_in)}</p>
                </div>
              </div>
              <div className="flex items-center gap-2 rounded-lg border p-3">
                <ArrowUpFromLine className="h-4 w-4 text-indigo-500" />
                <div>
                  <p className="text-xs text-muted-foreground">Total Tokens Out</p>
                  <p className="text-lg font-semibold">{formatNumber(allTime.tokens_out)}</p>
                </div>
              </div>
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
