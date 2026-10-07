import { useEffect, useState } from "react";
import {
  Banner,
  Button,
  HStack,
  MetadataList,
  MetadataListItem,
  Popover,
  ProgressBar,
  Text,
  VStack,
} from "@astryxdesign/core";
import { Gauge } from "lucide-react";
import type { Usage, UsageLimit, UsageWindow } from "./types";

/** Names a window by its length, such as "5h" or "Week". */
export function windowLabel(w: UsageWindow, fallback: string): string {
  const mins = w.windowDurationMins;
  if (!mins || mins <= 0) return fallback;
  const count = (n: number, one: string, many: string) =>
    n === 1 ? one : `${n} ${many}`;
  if (mins % 10080 === 0) return count(mins / 10080, "Week", "weeks");
  if (mins % 1440 === 0) return count(mins / 1440, "Day", "days");
  if (mins % 60 === 0) return `${mins / 60}h`;
  return `${mins}m`;
}

/** When a window starts over: a time today, else a weekday. */
export function resetLabel(w: UsageWindow, now = Date.now()): string | null {
  if (w.resetsAt == null) return null;
  const at = w.resetsAt * 1000;
  if (at <= now) return "resetting now";
  const date = new Date(at);
  if (at - now < 24 * 3600 * 1000)
    return `resets ${date.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })}`;
  return `resets ${date.toLocaleDateString(undefined, { weekday: "short" })}`;
}

const reachedLabels: Record<string, string> = {
  rate_limit_reached: "Your Codex usage limit is reached.",
  workspace_owner_credits_depleted: "Your workspace is out of Codex credits.",
  workspace_member_credits_depleted: "Your workspace is out of Codex credits.",
  workspace_owner_usage_limit_reached:
    "Your workspace's Codex usage limit is reached.",
  workspace_member_usage_limit_reached:
    "Your workspace's Codex usage limit is reached.",
};

/** Why reviews can't run right now, when a limit is reached. */
export function reachedMessage(usage: Usage): string | null {
  const type = [usage.rateLimits, ...usage.otherLimits].find(
    (l) => l.rateLimitReachedType,
  )?.rateLimitReachedType;
  if (!type) return null;
  return reachedLabels[type] ?? "A Codex usage limit is reached.";
}

const percent = (w: UsageWindow) => `${Math.round(w.usedPercent)}%`;

/** Each window's name and state, such as ["Week", "42% · resets Mon"]. */
function windowRows(limit: UsageLimit, now: number): [string, string][] {
  const windows: [UsageWindow | null | undefined, string][] = [
    [limit.secondary, "Week"],
    [limit.primary, "Short-term"],
  ];
  return windows
    .filter((entry): entry is [UsageWindow, string] => !!entry[0])
    .map(([w, fallback]) => [
      windowLabel(w, fallback),
      [percent(w), resetLabel(w, now)].filter(Boolean).join(" · "),
    ]);
}
const windowLines = (limit: UsageLimit, now: number) =>
  windowRows(limit, now).map(([label, state]) => `${label} ${state}`);

const capitalize = (text: string) =>
  text.charAt(0).toUpperCase() + text.slice(1);

/** The current time, refreshed each minute so reset times stay right. */
function useNow(fixed?: number): number {
  const [clock, setClock] = useState(() => Date.now());
  useEffect(() => {
    if (fixed !== undefined) return;
    const timer = setInterval(() => setClock(Date.now()), 60_000);
    return () => clearInterval(timer);
  }, [fixed]);
  return fixed ?? clock;
}

function limitName(limit: UsageLimit): string {
  return limit.limitName || limit.normalModelSlug || limit.limitId || "Limit";
}

function UsageDetails({ usage, now }: { usage: Usage; now: number }) {
  const main = usage.rateLimits;
  const credits = main.credits;
  const reached = reachedMessage(usage);
  return (
    <VStack gap={3} padding={3}>
      {reached && (
        <Banner
          status="warning"
          title={reached}
          description="Reviews that start now will stop until the limit resets."
        />
      )}
      <MetadataList title="Codex usage">
        {main.planType && (
          <MetadataListItem label="Plan">
            {capitalize(main.planType)}
          </MetadataListItem>
        )}
        {windowRows(main, now).map(([label, state]) => (
          <MetadataListItem key={label} label={label}>
            {state}
          </MetadataListItem>
        ))}
        {usage.otherLimits.map((limit) => (
          <MetadataListItem
            key={limit.limitId ?? limitName(limit)}
            label={limitName(limit)}
          >
            {windowLines(limit, now).join(", ")}
          </MetadataListItem>
        ))}
        {credits?.hasCredits && (
          <MetadataListItem label="Credits">
            {credits.unlimited ? "Unlimited" : (credits.balance ?? "Available")}
          </MetadataListItem>
        )}
        <MetadataListItem label="Updated">
          {new Date(usage.updatedAt).toLocaleTimeString(undefined, {
            hour: "numeric",
            minute: "2-digit",
          })}
        </MetadataListItem>
      </MetadataList>
    </VStack>
  );
}

/** How much of the Codex allowance is used, for the side navigation. */
export function UsageMeter({
  usage,
  now: fixedNow,
}: {
  usage?: Usage | null;
  /** A fixed time, for tests. */
  now?: number;
}) {
  const now = useNow(fixedNow);
  if (!usage) return null;
  const main = usage.rateLimits;
  const bar = main.secondary ?? main.primary;
  const lines = windowLines(main, now);
  const reached = reachedMessage(usage);
  return (
    <VStack gap={1}>
      {bar && (
        <ProgressBar
          label="Codex usage"
          isLabelHidden
          value={Math.min(100, Math.max(0, bar.usedPercent))}
          variant={
            reached ? "error" : bar.usedPercent >= 80 ? "warning" : "accent"
          }
        />
      )}
      <HStack gap={1} vAlign="center">
        <VStack gap={0}>
          {reached && (
            <Text type="supporting" color="secondary">
              Limit reached
            </Text>
          )}
          {lines.map((line) => (
            <Text key={line} type="supporting" color="secondary">
              {line}
            </Text>
          ))}
        </VStack>
        <Popover
          label="Codex usage details"
          placement="end"
          alignment="end"
          width={320}
          content={<UsageDetails usage={usage} now={now} />}
        >
          <Button
            label="Usage details"
            isIconOnly
            icon={<Gauge />}
            size="sm"
            variant="ghost"
          />
        </Popover>
      </HStack>
    </VStack>
  );
}
