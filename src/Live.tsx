import { useEffect, useRef, useState } from "react";
import {
  Button,
  Card,
  EmptyState,
  Grid,
  Heading,
  HStack,
  List,
  ListItem,
  SegmentedControl,
  SegmentedControlItem,
  StatusDot,
  Text,
  Timestamp,
  VStack,
} from "@astryxdesign/core";
import { Activity } from "lucide-react";
import { useOrbit } from "./store";
import { mainAction } from "./Loops";
import {
  activeStatuses,
  attentionAction,
  attentionLabels,
  sameRepo,
  statusLabels,
  type AttentionHandlers,
  type Review,
  type Snapshot,
} from "./types";

export type Unit = "hour" | "day";
export type Measure = "reviews" | "prs";
export interface Bucket {
  start: number;
  /** Count per repository, for reviews or distinct PRs. */
  counts: Record<string, number>;
  total: number;
}
const HOUR = 3600_000;
const spans: Record<Unit, number> = { hour: 24, day: 14 };

export function repoOf(prId: string) {
  const i = prId.lastIndexOf("#");
  return i < 0 ? prId : prId.slice(0, i);
}
function startOf(unit: Unit, t: number) {
  const d = new Date(t);
  d.setMinutes(0, 0, 0);
  if (unit === "day") d.setHours(0);
  return d.getTime();
}
function next(unit: Unit, t: number) {
  const d = new Date(t);
  if (unit === "hour") d.setHours(d.getHours() + 1);
  else d.setDate(d.getDate() + 1);
  return d.getTime();
}
/** Reviews started per hour (last 24) or per day (last 14), by repository. */
export function activity(
  reviews: Review[],
  unit: Unit,
  measure: Measure,
  now = Date.now(),
): Bucket[] {
  const starts: number[] = [startOf(unit, now)];
  while (starts.length < spans[unit]) {
    const d = new Date(starts[0]);
    if (unit === "hour") d.setHours(d.getHours() - 1);
    else d.setDate(d.getDate() - 1);
    starts.unshift(startOf(unit, d.getTime()));
  }
  const buckets = starts.map((start) => ({
    start,
    seen: new Set<string>(),
    counts: {} as Record<string, number>,
    total: 0,
  }));
  const end = next(unit, starts.at(-1)!);
  for (const r of reviews) {
    if (r.isReview === false) continue;
    const t = Date.parse(r.createdAt);
    if (!(t >= starts[0] && t < end)) continue;
    let i = starts.length - 1;
    while (starts[i] > t) i--;
    const b = buckets[i];
    if (measure === "prs") {
      if (b.seen.has(r.prId)) continue;
      b.seen.add(r.prId);
    }
    const repo = repoOf(r.prId);
    b.counts[repo] = (b.counts[repo] ?? 0) + 1;
    b.total++;
  }
  return buckets.map(({ start, counts, total }) => ({ start, counts, total }));
}

// Validated against the dark surface (lightness band, colour-blind
// separation, contrast) in this order; a sixth repository folds into Other.
const series = [
  "var(--color-data-blue-3)",
  "var(--color-data-orange-4)",
  "var(--color-data-teal-4)",
  "var(--color-data-categorical-indigo)",
  "var(--color-data-pink-4)",
];
const OTHER = "Other";
/** A colour per repository that stays put as the chart's window moves:
 * connected repositories in the order they were added, then the rest. */
export function repoColors(s: Snapshot) {
  const names: string[] = [];
  const add = (n: string) => {
    if (!names.some((m) => sameRepo(m, n))) names.push(n);
  };
  s.settings.repositories.forEach((r) => add(r.name));
  [...new Set(s.reviews.map((r) => repoOf(r.prId)))].sort().forEach(add);
  const colors = new Map<string, { series: string; color: string }>();
  names.forEach((n, i) =>
    colors.set(n.toLowerCase(), {
      series: i < series.length ? n : OTHER,
      color: series[i] ?? "var(--color-data-neutral)",
    }),
  );
  return (repo: string) =>
    colors.get(repo.toLowerCase()) ?? {
      series: OTHER,
      color: "var(--color-data-neutral)",
    };
}

/** Re-renders every `ms` while `on`, for live timers. */
function useNow(on: boolean, ms = 1000) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!on) return;
    const id = setInterval(() => setNow(Date.now()), ms);
    return () => clearInterval(id);
  }, [on, ms]);
  return now;
}
const reducedMotion = () =>
  window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? true;
/** Counts up to `value` from wherever it was. */
function useCountUp(value: number) {
  // The in-between number while counting; null shows `value` itself.
  const [frame, setFrame] = useState<number | null>(null);
  const from = useRef(value);
  useEffect(() => {
    const start = from.current;
    from.current = value;
    if (
      start === value ||
      reducedMotion() ||
      typeof requestAnimationFrame !== "function"
    )
      return;
    const t0 = performance.now();
    let id = 0;
    const step = (t: number) => {
      const k = Math.min(1, (t - t0) / 600);
      const eased = 1 - (1 - k) ** 3;
      setFrame(k < 1 ? Math.round(start + (value - start) * eased) : null);
      if (k < 1) id = requestAnimationFrame(step);
    };
    id = requestAnimationFrame(step);
    return () => {
      cancelAnimationFrame(id);
      setFrame(null);
    };
  }, [value]);
  return frame ?? value;
}
export function elapsed(ms: number) {
  const s = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const pad = (n: number) => String(n).padStart(2, "0");
  return h ? `${h}:${pad(m)}:${pad(s % 60)}` : `${m}:${pad(s % 60)}`;
}

interface Problem {
  key: string;
  title: string;
  detail: string;
  action?: { label: string; onClick: () => void };
  onOpen?: () => void;
}
/** Everything that needs a person, connections first. */
function problems(
  s: Snapshot,
  go: AttentionHandlers,
  run: (command: string, args: Record<string, unknown>) => void,
): Problem[] {
  const out: Problem[] = [];
  if (!s.codexReady)
    out.push({
      key: "codex",
      title: "Codex is disconnected",
      detail: s.codexError || "Reviews can't run until Codex reconnects.",
      action: { label: "Open settings", onClick: go.onSettings },
    });
  if (s.githubError)
    out.push({
      key: "github",
      title: "GitHub is not reachable",
      detail: s.githubError,
      action: { label: "Open settings", onClick: go.onSettings },
    });
  const known = [...s.prs, ...(s.historyPrs ?? [])];
  for (const a of s.attention ?? []) {
    const p = known.find((p) => p.id === a.prId);
    out.push({
      key: a.key,
      title: p?.title ?? a.repo ?? attentionLabels[a.kind],
      detail: [attentionLabels[a.kind], a.prId, a.reason]
        .filter(Boolean)
        .join(" · "),
      action: mainAction(s, a, go, run),
      onOpen: attentionAction(s, a, go),
    });
  }
  return out;
}

function Problems(go: AttentionHandlers) {
  const { snapshot: s, action } = useOrbit();
  const run = (command: string, args: Record<string, unknown>) =>
    void action(command, args);
  const items = problems(s, go, run);
  if (!items.length)
    return (
      <Card variant="green" padding={3}>
        <HStack gap={3} vAlign="center">
          <StatusDot variant="success" label="All clear" />
          <Text weight="medium">All clear. Nothing needs you.</Text>
        </HStack>
      </Card>
    );
  return (
    <Card
      variant="red"
      elevation="med"
      className="live-alert"
      padding={0}
      role="region"
      aria-label="Needs you"
    >
      <List
        hasDividers
        header={
          <HStack gap={3} vAlign="center" padding={3}>
            <StatusDot variant="error" label="Needs you" isPulsing />
            <Heading level={3}>
              {items.length === 1
                ? "1 thing needs you"
                : `${items.length} things need you`}
            </Heading>
          </HStack>
        }
      >
        {items.map((p) => (
          <ListItem
            key={p.key}
            label={
              <Text weight="medium" maxLines={2}>
                {p.title}
              </Text>
            }
            description={<Text maxLines={3}>{p.detail}</Text>}
            onClick={p.onOpen}
            endContent={
              p.action && (
                <Button
                  label={p.action.label}
                  size="sm"
                  variant="primary"
                  onClick={p.action.onClick}
                />
              )
            }
          />
        ))}
      </List>
    </Card>
  );
}

function Stat({
  label,
  value,
  tone,
}: {
  label: string;
  value: number;
  tone?: "accent" | "error";
}) {
  const shown = useCountUp(value);
  return (
    <Card padding={4} variant={tone === "error" && value ? "red" : "default"}>
      <VStack gap={1}>
        <Text type="supporting">{label}</Text>
        <HStack gap={2} vAlign="center">
          <Text type="display-2" hasTabularNumbers aria-label={String(value)}>
            {shown}
          </Text>
          {tone && value > 0 && (
            <StatusDot
              variant={tone}
              label={label}
              isPulsing={tone === "accent"}
            />
          )}
        </HStack>
      </VStack>
    </Card>
  );
}

const dots: Record<string, "accent" | "warning" | "neutral"> = {
  queued: "neutral",
  preparing: "accent",
  reviewing: "accent",
  awaitingApproval: "warning",
};
function Running({ go }: { go: AttentionHandlers }) {
  const { snapshot: s } = useOrbit();
  const active = s.reviews
    .filter((r) => activeStatuses.includes(r.status))
    .sort((a, b) => a.createdAt.localeCompare(b.createdAt));
  const now = useNow(active.length > 0);
  const known = [...s.prs, ...(s.historyPrs ?? [])];
  if (!active.length)
    return (
      <EmptyState
        icon={<Activity />}
        title="Nothing running"
        description={
          s.settings.automatic
            ? "Autopilot is watching for new pushes."
            : "Turn on automatic reviews to let Orbit pick up new pushes."
        }
      />
    );
  return (
    <List hasDividers aria-label="Running reviews">
      {active.map((r) => {
        const p = known.find((p) => p.id === r.prId);
        const approval = s.approvals.find((a) => a.reviewId === r.id);
        return (
          <ListItem
            key={r.id}
            onClick={p ? () => go.onChoose(p, r) : undefined}
            startContent={
              <StatusDot
                variant={dots[r.status] ?? "neutral"}
                label={statusLabels[r.status]}
                isPulsing={r.status !== "queued"}
              />
            }
            label={
              <Text weight="medium" maxLines={1}>
                {p?.title ?? r.prId}
              </Text>
            }
            description={
              <Text color="secondary" maxLines={1}>
                {[
                  r.prId,
                  statusLabels[r.status],
                  r.automatic ? "Autopilot" : "Started by you",
                  [r.model, r.effort].filter(Boolean).join(" · "),
                ]
                  .filter(Boolean)
                  .join(" · ")}
              </Text>
            }
            endContent={
              approval ? (
                <Button
                  label="Approve or deny"
                  size="sm"
                  variant="primary"
                  onClick={() => go.onApproval(r.id, approval.id)}
                />
              ) : (
                <Text
                  type="code"
                  hasTabularNumbers
                  aria-label={`Running for ${elapsed(now - Date.parse(r.createdAt))}`}
                >
                  {elapsed(now - Date.parse(r.createdAt))}
                </Text>
              )
            }
          />
        );
      })}
    </List>
  );
}

function bucketLabel(unit: Unit, t: number) {
  return unit === "hour"
    ? new Date(t).toLocaleTimeString(undefined, { hour: "numeric" })
    : new Date(t).toLocaleDateString(undefined, {
        weekday: "short",
        day: "numeric",
      });
}
// Rounded top corners; the base sits flat on the baseline.
function topRounded(x: number, y: number, w: number, h: number, r: number) {
  const k = Math.min(r, h, w / 2);
  return `M${x},${y + h}V${y + k}Q${x},${y} ${x + k},${y}H${x + w - k}Q${x + w},${y} ${x + w},${y + k}V${y + h}Z`;
}

const H = 200;
const AXIS = 24;
const LEFT = 28;

// The chart is an inline SVG: Astryx has no chart component, so this and the
// diff's line grid in Files.tsx are the two raw-element exceptions to
// AGENTS.md. Colours, type and motion still come from tokens.
function ActivityChart({
  buckets,
  unit,
  measure,
  colorOf,
}: {
  buckets: Bucket[];
  unit: Unit;
  measure: Measure;
  colorOf: ReturnType<typeof repoColors>;
}) {
  const [hover, setHover] = useState<number | null>(null);
  // Drawn at its rendered width so labels keep the page's type size.
  const ref = useRef<SVGSVGElement>(null);
  const [W, setW] = useState(720);
  useEffect(() => {
    const svg = ref.current;
    if (!svg) return;
    const observer = new ResizeObserver(([e]) => {
      if (e?.contentRect.width) setW(Math.round(e.contentRect.width));
    });
    observer.observe(svg);
    return () => observer.disconnect();
  }, []);
  const max = Math.max(1, ...buckets.map((b) => b.total));
  const top = max <= 4 ? max : Math.ceil(max / 5) * 5;
  const plot = H - AXIS;
  const slot = (W - LEFT) / buckets.length;
  const bar = Math.max(4, slot * 0.6);
  const noun = measure === "reviews" ? "review" : "PR";
  const describe = (b: Bucket) =>
    `${bucketLabel(unit, b.start)} · ${b.total} ${noun}${b.total === 1 ? "" : "s"}` +
    Object.entries(b.counts)
      .sort(([a], [b]) => colorOrder(colorOf, a) - colorOrder(colorOf, b))
      .map(([repo, n]) => ` · ${repo} ${n}`)
      .join("");
  const total = buckets.reduce((n, b) => n + b.total, 0);
  const every = unit === "hour" ? 3 : 2;
  const focused = hover == null ? null : buckets[hover];
  return (
    <VStack gap={2}>
      <Text type="supporting" aria-live="polite">
        {focused
          ? describe(focused)
          : `${total} ${noun}${total === 1 ? "" : "s"} in the last ${unit === "hour" ? "24 hours" : "14 days"}. Hover a bar for details.`}
      </Text>
      <svg
        ref={ref}
        className="live-chart"
        height={H}
        viewBox={`0 0 ${W} ${H}`}
        role="img"
        aria-label={`${measure === "reviews" ? "Reviews" : "Pull requests reviewed"} per ${unit}`}
        onMouseLeave={() => setHover(null)}
      >
        {[0, top / 2, top].map((v) => {
          const y = plot - (v / top) * (plot - 8);
          return (
            <g key={v}>
              <line
                x1={LEFT}
                x2={W}
                y1={y}
                y2={y}
                className="live-chart-grid"
              />
              {Number.isInteger(v) && (
                <text x={LEFT - 6} y={y + 4} textAnchor="end">
                  {v}
                </text>
              )}
            </g>
          );
        })}
        {buckets.map((b, i) => {
          const x = LEFT + i * slot + (slot - bar) / 2;
          let y = plot;
          const segments = Object.entries(b.counts).sort(
            ([a], [b]) => colorOrder(colorOf, a) - colorOrder(colorOf, b),
          );
          return (
            <g
              key={`${unit}-${measure}-${b.start}`}
              className="live-bar"
              data-dim={hover != null && hover !== i ? "" : undefined}
              style={{ animationDelay: `${i * 18}ms` }}
            >
              {segments.map(([repo, n], j) => {
                const h = (n / top) * (plot - 8);
                y -= h;
                const gap = j < segments.length - 1 ? 2 : 0;
                const last = j === segments.length - 1;
                return (
                  <path
                    key={repo}
                    d={
                      last
                        ? topRounded(x, y, bar, h, 4)
                        : `M${x},${y + gap}h${bar}V${y + h}h${-bar}Z`
                    }
                    style={{ fill: colorOf(repo).color }}
                  />
                );
              })}
              <rect
                x={LEFT + i * slot}
                y={0}
                width={slot}
                height={plot}
                className="live-chart-hit"
                tabIndex={0}
                aria-label={describe(b)}
                onMouseEnter={() => setHover(i)}
                onFocus={() => setHover(i)}
                onBlur={() => setHover(null)}
              />
            </g>
          );
        })}
        {buckets.map(
          (b, i) =>
            i % every === (buckets.length - 1) % every && (
              <text
                key={b.start}
                x={LEFT + (i + 0.5) * slot}
                y={H - 6}
                textAnchor="middle"
              >
                {bucketLabel(unit, b.start)}
              </text>
            ),
        )}
      </svg>
    </VStack>
  );
}
function seriesIndex(color: string) {
  const i = series.indexOf(color);
  return i < 0 ? series.length : i;
}
function colorOrder(colorOf: ReturnType<typeof repoColors>, repo: string) {
  return seriesIndex(colorOf(repo).color);
}

function Legend({
  repos,
  colorOf,
}: {
  repos: string[];
  colorOf: ReturnType<typeof repoColors>;
}) {
  const entries = new Map<string, string>();
  for (const r of repos) {
    const c = colorOf(r);
    entries.set(c.series, c.color);
  }
  if (!entries.size) return null;
  return (
    <HStack gap={4} wrap="wrap" aria-label="Legend">
      {[...entries]
        .sort(([, a], [, b]) => seriesIndex(a) - seriesIndex(b))
        .map(([name, color]) => (
          <HStack key={name} gap={1} vAlign="center">
            <svg className="live-swatch" viewBox="0 0 10 10" aria-hidden>
              <rect width={10} height={10} rx={2} style={{ fill: color }} />
            </svg>
            <Text type="supporting">{name}</Text>
          </HStack>
        ))}
    </HStack>
  );
}

function Charts() {
  const { snapshot: s } = useOrbit();
  const [unit, setUnit] = useState<Unit>("hour");
  const [measure, setMeasure] = useState<Measure>("reviews");
  // Refresh the window on the minute so a new hour or day slides in.
  const now = useNow(true, 60_000);
  const buckets = activity(s.reviews, unit, measure, now);
  const colorOf = repoColors(s);
  const repos = [...new Set(buckets.flatMap((b) => Object.keys(b.counts)))];
  return (
    <Card padding={4}>
      <VStack gap={3}>
        <HStack gap={3} vAlign="center" hAlign="between" wrap="wrap">
          <Heading level={3}>Activity</Heading>
          <HStack gap={2} wrap="wrap">
            <SegmentedControl
              label="Count"
              size="sm"
              value={measure}
              onChange={(v) => setMeasure(v as Measure)}
            >
              <SegmentedControlItem value="reviews" label="Reviews" />
              <SegmentedControlItem value="prs" label="PRs" />
            </SegmentedControl>
            <SegmentedControl
              label="Per"
              size="sm"
              value={unit}
              onChange={(v) => setUnit(v as Unit)}
            >
              <SegmentedControlItem value="hour" label="Per hour" />
              <SegmentedControlItem value="day" label="Per day" />
            </SegmentedControl>
          </HStack>
        </HStack>
        <ActivityChart
          buckets={buckets}
          unit={unit}
          measure={measure}
          colorOf={colorOf}
        />
        <Legend repos={repos} colorOf={colorOf} />
      </VStack>
    </Card>
  );
}

/** What autopilot is doing now, how busy it has been, and what is wrong. */
export function Live(go: AttentionHandlers) {
  const { snapshot: s } = useOrbit();
  const now = useNow(true, 60_000);
  const day = s.reviews.filter(
    (r) => r.isReview !== false && now - Date.parse(r.createdAt) < 24 * HOUR,
  );
  const watching = s.settings.repositories.filter((r) => r.automatic).length;
  return (
    <VStack gap={6} padding={6} maxWidth={1100} className="content-page">
      <VStack gap={2}>
        <HStack gap={3} vAlign="center">
          <StatusDot
            variant={s.settings.automatic ? "success" : "neutral"}
            label={s.settings.automatic ? "Autopilot on" : "Autopilot off"}
            isPulsing={s.settings.automatic}
          />
          <Heading level={2}>Live</Heading>
        </HStack>
        <HStack gap={1} vAlign="center" wrap="wrap">
          <Text color="secondary">
            {s.settings.automatic
              ? `Autopilot is on, watching ${watching} ${watching === 1 ? "repository" : "repositories"}.`
              : "Autopilot is off."}
          </Text>
          {s.lastPoll && (
            <>
              <Text color="secondary">Last checked GitHub</Text>
              <Timestamp
                value={s.lastPoll}
                format="relative_short"
                isLive
                type="body"
              />
            </>
          )}
        </HStack>
      </VStack>
      <Problems {...go} />
      <Grid columns={{ minWidth: 160, repeat: "fit" }} gap={3}>
        <Stat
          label="Running now"
          value={
            s.reviews.filter(
              (r) => activeStatuses.includes(r.status) && r.status !== "queued",
            ).length
          }
          tone="accent"
        />
        <Stat
          label="Queued"
          value={s.reviews.filter((r) => r.status === "queued").length}
        />
        <Stat label="Reviews, last 24h" value={day.length} />
        <Stat
          label="PRs, last 24h"
          value={new Set(day.map((r) => r.prId)).size}
        />
        <Stat
          label="Failed, last 24h"
          value={day.filter((r) => r.status === "failed").length}
          tone="error"
        />
      </Grid>
      <VStack gap={3}>
        <Heading level={3}>Running now</Heading>
        <Running go={go} />
      </VStack>
      <Charts />
    </VStack>
  );
}
