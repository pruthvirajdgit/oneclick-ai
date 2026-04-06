import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { CalendarClock, Plus } from "lucide-react";

const schedules = [
  { name: "Daily report generation", cron: "0 9 * * *", status: "active" },
  { name: "Weekly data sync", cron: "0 0 * * MON", status: "active" },
  { name: "Monthly invoice summary", cron: "0 8 1 * *", status: "paused" },
];

export default function SchedulesPage() {
  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold text-foreground">Schedules</h1>
          <p className="text-muted-foreground">
            Manage your automated tasks
          </p>
        </div>
        <Button>
          <Plus className="mr-2 h-4 w-4" />
          New Schedule
        </Button>
      </div>

      <div className="space-y-3">
        {schedules.map((schedule) => (
          <Card key={schedule.name} className="shadow-sm">
            <CardHeader className="flex flex-row items-center justify-between py-3">
              <div className="flex items-center gap-3">
                <CalendarClock className="h-5 w-5 text-primary" />
                <div>
                  <CardTitle className="text-base">{schedule.name}</CardTitle>
                  <p className="text-xs text-muted-foreground font-mono">
                    {schedule.cron}
                  </p>
                </div>
              </div>
              <Badge
                variant={schedule.status === "active" ? "default" : "secondary"}
              >
                {schedule.status}
              </Badge>
            </CardHeader>
            <CardContent className="pt-0">
              <p className="text-sm text-muted-foreground">
                Schedule details and configuration coming soon.
              </p>
            </CardContent>
          </Card>
        ))}
      </div>
    </div>
  );
}
