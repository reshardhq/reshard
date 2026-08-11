import { AreaChart } from "@/components/dither-kit/area-chart";
import { Area } from "@/components/dither-kit/area";
import { XAxis } from "@/components/dither-kit/x-axis";
import { YAxis } from "@/components/dither-kit/y-axis";
import { Legend } from "@/components/dither-kit/legend";
import { Tooltip } from "@/components/dither-kit/tooltip";

/* messages handled per agent, last 7 days — dithered */

const data = [
  { day: "Mon", claude: 34, kimi: 18 },
  { day: "Tue", claude: 41, kimi: 22 },
  { day: "Wed", claude: 28, kimi: 31 },
  { day: "Thu", claude: 52, kimi: 26 },
  { day: "Fri", claude: 61, kimi: 38 },
  { day: "Sat", claude: 22, kimi: 41 },
  { day: "Sun", claude: 18, kimi: 12 },
];

const config = {
  claude: { label: "claude-main", color: "orange" },
  kimi: { label: "kimi-research", color: "blue" },
} as const;

export function DitherChartDemo() {
  return (
    <div className="w-full max-w-105 rounded-card bg-surface p-3 shadow-hairline">
      <p className="mb-2 text-[12px] font-medium text-ink">
        Agent throughput — messages handled, last 7 days
      </p>
      <div className="h-52">
        <AreaChart data={data} config={config} bloom="aura">
          <XAxis dataKey="day" />
          <YAxis />
          <Legend isClickable />
          <Tooltip labelKey="day" />
          <Area dataKey="claude" variant="gradient" />
          <Area dataKey="kimi" variant="dotted" />
        </AreaChart>
      </div>
    </div>
  );
}
