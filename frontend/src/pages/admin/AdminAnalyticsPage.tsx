import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Users, Bot, Activity, Zap, Coins } from "lucide-react";

const stats = [
  { label: "Total Users", value: "8", icon: Users, color: "text-[#8b5cf6]", bg: "bg-[#8b5cf6]/10" },
  { label: "Total Agents", value: "10", icon: Bot, color: "text-blue-600", bg: "bg-blue-100" },
  { label: "Active Agents", value: "6", icon: Activity, color: "text-emerald-600", bg: "bg-emerald-100" },
  { label: "Requests Today", value: "1,247", icon: Zap, color: "text-orange-600", bg: "bg-orange-100" },
  { label: "Total Tokens Used", value: "2.4M", icon: Coins, color: "text-amber-600", bg: "bg-amber-100" },
];

export default function AdminAnalyticsPage() {
  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold text-foreground">Analytics</h1>
        <p className="text-sm text-muted-foreground mt-1">
          Platform-wide usage overview
        </p>
      </div>

      {/* Stat cards */}
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-5 gap-4">
        {stats.map(({ label, value, icon: Icon, color, bg }) => (
          <Card key={label}>
            <CardContent className="flex items-center gap-4 p-5">
              <div className={`flex h-11 w-11 shrink-0 items-center justify-center rounded-lg ${bg}`}>
                <Icon className={`h-5 w-5 ${color}`} />
              </div>
              <div>
                <p className="text-sm text-muted-foreground">{label}</p>
                <p className="text-2xl font-bold text-foreground">{value}</p>
              </div>
            </CardContent>
          </Card>
        ))}
      </div>

      {/* Chart placeholders */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        <Card>
          <CardHeader>
            <CardTitle className="text-base">Requests Over Time</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="flex h-56 items-center justify-center rounded-lg border-2 border-dashed border-border bg-muted/30">
              <p className="text-sm text-muted-foreground">
                📊 Chart coming soon
              </p>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-base">Token Usage by Model</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="flex h-56 items-center justify-center rounded-lg border-2 border-dashed border-border bg-muted/30">
              <p className="text-sm text-muted-foreground">
                📊 Chart coming soon
              </p>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-base">User Growth</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="flex h-56 items-center justify-center rounded-lg border-2 border-dashed border-border bg-muted/30">
              <p className="text-sm text-muted-foreground">
                📊 Chart coming soon
              </p>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-base">Agent Status Distribution</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="flex h-56 items-center justify-center rounded-lg border-2 border-dashed border-border bg-muted/30">
              <p className="text-sm text-muted-foreground">
                📊 Chart coming soon
              </p>
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
