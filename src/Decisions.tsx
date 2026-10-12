import { useEffect, useMemo, useState } from "react";
import {
  Banner,
  Button,
  Card,
  EmptyState,
  Grid,
  Heading,
  HStack,
  List,
  ListItem,
  MetadataList,
  MetadataListItem,
  ProgressBar,
  Selector,
  Spinner,
  StackItem,
  StatusDot,
  Tab,
  TabList,
  Text,
  TextInput,
  Timestamp,
  Token,
  VStack,
} from "@astryxdesign/core";
import {
  Table,
  pixel,
  proportional,
  type TableColumn,
} from "@astryxdesign/core/Table";
import {
  ArrowLeft,
  ArrowUpRight,
  RefreshCw,
  Scale,
  Search,
} from "lucide-react";
import { live, openLink, useOrbit } from "./store";
import { categoryLabels, priorStatusLabels, sameRepo } from "./types";
import { priorityColors } from "./Findings";
import { SafeMarkdown } from "./SafeMarkdown";
import {
  decisionKindLabels,
  decisionKinds,
  duration,
  finalStatusColors,
  finalStatusLabels,
  finalStatuses,
  flagLabels,
  gist,
  health,
  historyKey,
  madeByLabels,
  pushback,
  searchHistories,
  shortRef,
  stateColor,
  stateLabel,
  verdictColors,
  verdictLabels,
  worthALook,
  type Decisions,
  type FinalStatus,
  type FlagRow,
  type History,
  type LedgerClaim,
  type LedgerRound,
  type Trajectory,
  type WriteUp,
} from "./decisions";

type View = "story" | "pushback" | "health";
const PAGE = 50;
const FLAGS = 5;

function PriorityToken({ priority }: { priority: number }) {
  return (
    <Token
      label={`P${priority}`}
      color={priorityColors[priority] ?? "gray"}
      size="sm"
    />
  );
}
function StatusToken({ status }: { status: FinalStatus }) {
  return (
    <Token
      label={finalStatusLabels[status]}
      color={finalStatusColors[status]}
      size="sm"
    />
  );
}
function StateToken({ pr }: { pr: History }) {
  return <Token label={stateLabel(pr)} color={stateColor(pr)} size="sm" />;
}

/** Red for something to decide, grey for something good to know. */
function FlagDot({ attention }: { attention: "act" | "look" }) {
  return (
    <StatusDot
      variant={attention === "act" ? "error" : "neutral"}
      label={attention === "act" ? "Needs a decision" : "Good to know"}
    />
  );
}

/** Blocking findings round by round, such as "3 → 1 → 0", as the Review
 * loops page shows them. */
export function roundStrip(pr: History) {
  return pr.rounds
    .map((r) =>
      r.verdict === "unknown"
        ? "?"
        : String(
            r.blocking ??
              (r.verdict === "clean"
                ? 0
                : r.findings.filter((f) => f.blocking !== false).length),
          ),
    )
    .join(" → ");
}
const plural = (n: number, one: string, many = `${one}s`) =>
  `${n} ${n === 1 ? one : many}`;

/** What the builder said about a finding, in a few words. */
function said(claim: LedgerClaim | null | undefined) {
  if (!claim) return "";
  if (claim.kind === "fixed")
    return claim.commit ? `Fixed in ${claim.commit.slice(0, 7)}` : "Fixed";
  return claim.reason ? `Declined: ${claim.reason}` : "Declined";
}

/** How the latest read from GitHub went. */
function ReadStatus() {
  const ledger = useOrbit((state) => state.snapshot.ledger);
  const repos = useOrbit((state) => state.snapshot.settings.repositories);
  if (!live) return null;
  const progress = ledger?.progress;
  const writing = ledger?.writing;
  const failed = (ledger?.repos ?? []).filter(
    (r) => r.error && repos.some((c) => sameRepo(c.name, r.repo)),
  );
  const last = (ledger?.repos ?? [])
    .map((r) => r.refreshedAt)
    .filter((t): t is string => !!t)
    .sort()
    .at(-1);
  return (
    <VStack gap={2}>
      {ledger?.refreshing ? (
        <HStack gap={2} vAlign="center">
          <Spinner size="sm" label="Reading" />
          <Text type="supporting" aria-live="polite">
            {progress && progress.total > 0
              ? `Reading ${progress.repo} from GitHub · ${progress.done} of ${progress.total}`
              : "Reading pull requests from GitHub…"}
          </Text>
        </HStack>
      ) : last ? (
        <HStack gap={1} vAlign="center">
          <Text type="supporting">Read from GitHub</Text>
          <Timestamp value={last} format="relative_short" />
        </HStack>
      ) : null}
      {writing && (
        <HStack gap={2} vAlign="center">
          <Spinner size="sm" label="Writing up" />
          <Text type="supporting" aria-live="polite">
            {`Writing up ${writing.repo.split("/")[1] ?? writing.repo}#${writing.number} in plain words · ${writing.done + 1} of ${writing.total}`}
          </Text>
        </HStack>
      )}
      {ledger?.writeError && (
        <Banner
          status="warning"
          title="Write-ups"
          description={ledger.writeError}
          collapsible={false}
        />
      )}
      {failed.map((r) => (
        <Banner
          key={r.repo}
          status="warning"
          title={`Couldn't read ${r.repo}`}
          description={`${r.error} Orbit tries again in a few minutes and keeps what it already had.`}
          collapsible={false}
        />
      ))}
    </VStack>
  );
}

/** Flags across every pull request, those that need a decision first. */
function WorthALook({
  rows,
  onOpen,
}: {
  rows: FlagRow[];
  onOpen: (pr: History) => void;
}) {
  const [all, setAll] = useState(false);
  const shown = all ? rows : rows.slice(0, FLAGS);
  return (
    <List hasDividers header={<Heading level={3}>Worth a look</Heading>}>
      {rows.length === 0 && (
        <Text color="secondary">Nothing needs a second look.</Text>
      )}
      {shown.map(({ pr, flag }, i) => (
        <ListItem
          key={`${historyKey(pr)}-${flag.kind}-${flag.findingId ?? i}`}
          label={flagLabels[flag.kind]}
          startContent={<FlagDot attention={flag.attention} />}
          description={
            <VStack gap={1}>
              <Text>{flag.text}</Text>
              <Text type="supporting">{`${shortRef(pr)} · ${pr.title}`}</Text>
            </VStack>
          }
          onClick={() => onOpen(pr)}
        />
      ))}
      {rows.length > FLAGS && (
        <HStack>
          <StackItem>
            <Button
              label={all ? "Show fewer" : `Show ${rows.length - FLAGS} more`}
              size="sm"
              variant="ghost"
              onClick={() => setAll(!all)}
            />
          </StackItem>
        </HStack>
      )}
    </List>
  );
}

/** Each pull request as a chapter, newest first. */
function Story({
  prs,
  onOpen,
}: {
  prs: History[];
  onOpen: (pr: History) => void;
}) {
  const [shown, setShown] = useState(PAGE);
  if (!prs.length)
    return <Text color="secondary">No pull requests match.</Text>;
  return (
    <VStack gap={3}>
      <List hasDividers aria-label="Pull requests">
        {prs.slice(0, shown).map((pr) => {
          const line = gist(pr);
          const m = pr.metrics;
          return (
            <ListItem
              key={historyKey(pr)}
              label={pr.title}
              startContent={<StateToken pr={pr} />}
              endContent={
                pr.rounds.length > 0 ? (
                  <Text
                    type="supporting"
                    hasTabularNumbers
                    aria-label={`Blocking findings by round: ${roundStrip(pr)}`}
                  >
                    {roundStrip(pr)}
                  </Text>
                ) : undefined
              }
              description={
                <VStack gap={1}>
                  {line && <Text color="secondary">{line}</Text>}
                  <HStack gap={1} vAlign="center" wrap="wrap">
                    <Text type="supporting">
                      {`${shortRef(pr)} · ${
                        pr.rounds.length
                          ? `${plural(m.rounds, "round")} · ${plural(m.findings, "finding")}`
                          : "no review"
                      } ·`}
                    </Text>
                    <Timestamp
                      value={pr.mergedAt ?? pr.updatedAt}
                      format="relative_short"
                    />
                  </HStack>
                </VStack>
              }
              onClick={() => onOpen(pr)}
            />
          );
        })}
      </List>
      {prs.length > shown && (
        <HStack>
          <StackItem>
            <Button
              label={`Show ${Math.min(PAGE, prs.length - shown)} more`}
              size="sm"
              onClick={() => setShown(shown + PAGE)}
            />
          </StackItem>
        </HStack>
      )}
    </VStack>
  );
}

/** Every finding the builder declined, with the reason and the reviewer's
 * answer. */
function Pushback({
  prs,
  onOpen,
}: {
  prs: History[];
  onOpen: (pr: History) => void;
}) {
  const rows = pushback(prs);
  if (!rows.length)
    return (
      <Text color="secondary">
        The builder hasn’t declined any findings in these pull requests.
      </Text>
    );
  return (
    <List hasDividers aria-label="Declined findings">
      {rows.map(({ pr, trajectory: t, reason, ruling }) => (
        <ListItem
          key={`${historyKey(pr)}-${t.findingId}`}
          label={t.shortId ? `${t.shortId} · ${t.title}` : t.title}
          startContent={<StatusToken status={t.finalStatus} />}
          description={
            <VStack gap={1}>
              <Text>
                {reason
                  ? `The builder said: ${reason}`
                  : "The builder declined it without a reason Orbit could read."}
              </Text>
              <Text color="secondary">
                {ruling
                  ? `The reviewer, round ${ruling.round}: ${priorStatusLabels[ruling.status]}${ruling.note ? `. ${ruling.note}` : ""}`
                  : "The reviewer hasn’t ruled on it yet."}
              </Text>
              <Text type="supporting">{`${shortRef(pr)} · ${pr.title}`}</Text>
            </VStack>
          }
          onClick={() => onOpen(pr)}
        />
      ))}
    </List>
  );
}

function Tile({
  label,
  value,
  detail,
}: {
  label: string;
  value: string;
  detail?: string;
}) {
  return (
    <Card padding={4}>
      <VStack gap={1}>
        <Text type="supporting">{label}</Text>
        <Text type="display-2" hasTabularNumbers>
          {value}
        </Text>
        {detail && <Text type="supporting">{detail}</Text>}
      </VStack>
    </Card>
  );
}

interface RoundsRow extends Record<string, unknown> {
  label: string;
  count: number;
}
interface PriorityRow extends Record<string, unknown> {
  priority: number;
  total: number;
  counts: Record<FinalStatus, number>;
}
const percent = (n: number, of: number) =>
  of ? `${Math.round((n / of) * 100)}%` : "–";

/** How reviews went across the pull requests: headline numbers, then two
 * charts, each with its numbers in a table underneath. */
function ReviewHealth({ prs }: { prs: History[] }) {
  const h = useMemo(() => health(prs), [prs]);
  if (!h.reviewed)
    return (
      <Text color="secondary">
        None of these pull requests has had a review round yet.
      </Text>
    );
  const roundColumns: TableColumn<RoundsRow>[] = [
    {
      key: "label",
      header: "Rounds",
      width: proportional(1),
      renderCell: (r) => <Text>{r.label}</Text>,
    },
    {
      key: "count",
      header: "Pull requests",
      width: pixel(120),
      align: "end",
      renderCell: (r) => <Text hasTabularNumbers>{r.count}</Text>,
    },
    {
      key: "share",
      header: "Share",
      width: pixel(88),
      align: "end",
      renderCell: (r) => (
        <Text hasTabularNumbers>{percent(r.count, h.reviewed)}</Text>
      ),
    },
  ];
  const priorityColumns: TableColumn<PriorityRow>[] = [
    {
      key: "priority",
      header: "Priority",
      width: pixel(88),
      renderCell: (r) => <PriorityToken priority={r.priority} />,
    },
    ...finalStatuses.map((status): TableColumn<PriorityRow> => ({
      key: status,
      header: finalStatusLabels[status],
      width: proportional(1),
      align: "end",
      renderCell: (r) => <Text hasTabularNumbers>{r.counts[status]}</Text>,
    })),
    {
      key: "total",
      header: "Total",
      width: pixel(72),
      align: "end",
      renderCell: (r) => (
        <Text weight="medium" hasTabularNumbers>
          {r.total}
        </Text>
      ),
    },
  ];
  const maxPriority = Math.max(1, ...h.byPriority.map((p) => p.total));
  return (
    <VStack gap={6}>
      <Grid columns={{ minWidth: 180, repeat: "fit" }} gap={3}>
        <Tile
          label="Pull requests reviewed"
          value={String(h.reviewed)}
          detail={`${plural(h.rounds, "round")} in all`}
        />
        <Tile
          label="Average rounds"
          value={(h.rounds / h.reviewed).toFixed(1)}
          detail="per reviewed pull request"
        />
        <Tile
          label="Clean on the first round"
          value={percent(h.cleanFirst, h.reviewed)}
          detail={`${h.cleanFirst} of ${h.reviewed}`}
        />
        <Tile
          label="Findings fixed"
          value={percent(h.fixed, h.findings)}
          detail={`${h.fixed} of ${plural(h.findings, "finding")}`}
        />
        <Tile
          label="Findings declined"
          value={String(h.declined)}
          detail={`${h.disputed} still disputed`}
        />
        <Tile
          label="Merged with a finding open"
          value={String(h.openAtMerge)}
          detail={h.openAtMerge ? "see Worth a look" : "none"}
        />
        <Tile
          label="Time to a clean round"
          value={h.medianToClean == null ? "–" : duration(h.medianToClean)}
          detail="middle value, from opening"
        />
      </Grid>
      <Card padding={4}>
        <VStack gap={3}>
          <Heading level={3}>Rounds per pull request</Heading>
          <VStack gap={2} aria-hidden>
            {h.roundsPerPr.map((r) => (
              <ProgressBar
                key={r.label}
                label={`${r.label} ${r.label === "1" ? "round" : "rounds"}`}
                value={r.count}
                max={h.reviewed}
                variant="accent"
                hasValueLabel
                formatValueLabel={(v) => plural(v, "pull request")}
              />
            ))}
          </VStack>
          <Table
            data={h.roundsPerPr}
            columns={roundColumns}
            idKey="label"
            density="compact"
          />
        </VStack>
      </Card>
      <Card padding={4}>
        <VStack gap={3}>
          <Heading level={3}>Findings by priority</Heading>
          <VStack gap={2} aria-hidden>
            {h.byPriority.map((p) => (
              <ProgressBar
                key={p.priority}
                label={`P${p.priority}`}
                value={p.total}
                max={maxPriority}
                variant={p.priority <= 1 ? "warning" : "neutral"}
                hasValueLabel
                formatValueLabel={(v) =>
                  `${plural(v, "finding")} · ${percent(p.counts.fixed, v)} fixed`
                }
              />
            ))}
          </VStack>
          <Table
            data={h.byPriority}
            columns={priorityColumns}
            idKey="priority"
            density="compact"
          />
        </VStack>
      </Card>
    </VStack>
  );
}

interface FindingRow extends Record<string, unknown> {
  id: string;
  t: Trajectory;
}
/** A round of review: its verdict, who reviewed, and what it found. */
function RoundItem({ round }: { round: LedgerRound }) {
  const n = round.round ?? "?";
  const reviewer =
    round.reviewer ?? (round.format === "milo" ? "Milo" : "Orbit");
  const found = round.findings.length;
  return (
    <ListItem
      label={`Round ${n} · ${verdictLabels[round.verdict]}`}
      startContent={
        <Token label={`R${n}`} color={verdictColors[round.verdict]} size="sm" />
      }
      description={
        <VStack gap={1}>
          <HStack gap={1} vAlign="center" wrap="wrap">
            <Text type="supporting">
              {`${reviewer}${round.headSha ? ` · ${round.headSha.slice(0, 7)}` : ""} · ${
                found ? plural(found, "new finding") : "no new findings"
              }${round.blocking ? ` · ${round.blocking} blocking` : ""} ·`}
            </Text>
            <Timestamp value={round.createdAt} format="relative_short" />
          </HStack>
          {round.summary && <Text color="secondary">{round.summary}</Text>}
          {round.earlierFindings.map((e) => (
            <Text key={e.id} type="supporting">
              {`${e.shortId ?? e.id} ${e.title}: ${
                e.status ? priorStatusLabels[e.status] : "not ruled on"
              }${e.note ? `. ${e.note}` : ""}`}
            </Text>
          ))}
          {round.notChecked.length > 0 && (
            <Text type="supporting">
              {`Not checked: ${round.notChecked.join("; ")}`}
            </Text>
          )}
        </VStack>
      }
      onClick={round.url ? () => openLink(round.url) : undefined}
    />
  );
}

/** One pull request's whole story. */
/** A merged or closed pull request as the writer summed it up, decisions
 * grouped by kind, each linking to where it was said. */
function InPlainWords({ w }: { w: WriteUp }) {
  const groups = decisionKinds
    .map((kind) => [kind, w.decisions.filter((d) => d.kind === kind)] as const)
    .filter(([, ds]) => ds.length > 0);
  return (
    <VStack gap={3}>
      <Heading level={3}>In plain words</Heading>
      <Text>{w.summary}</Text>
      {groups.map(([kind, ds]) => (
        <List
          key={kind}
          hasDividers
          header={<Text weight="medium">{decisionKindLabels[kind]}</Text>}
        >
          {ds.map((d, i) => (
            <ListItem
              key={`${kind}-${i}`}
              label={<Text>{d.statement}</Text>}
              startContent={
                d.attention ? <FlagDot attention={d.attention} /> : undefined
              }
              endContent={<ArrowUpRight size={16} aria-hidden />}
              onClick={() => openLink(d.url)}
              description={
                <VStack gap={1}>
                  {d.detail && <Text>{d.detail}</Text>}
                  {d.quote && <Text color="secondary">{`“${d.quote}”`}</Text>}
                  {d.attention && d.attentionReason && (
                    <Text>{d.attentionReason}</Text>
                  )}
                  {d.supersedes && (
                    <Text color="secondary">{`Changes an earlier rule: ${d.supersedes}`}</Text>
                  )}
                  <HStack gap={1} wrap="wrap">
                    <Token label={d.area} size="sm" />
                    <Token label={madeByLabels[d.madeBy]} size="sm" />
                  </HStack>
                </VStack>
              }
            />
          ))}
        </List>
      ))}
      <HStack gap={1} vAlign="center" wrap="wrap">
        <Text type="supporting" color="secondary">
          {`Written by ${w.writer} (${w.model})`}
        </Text>
        <Timestamp value={w.writtenAt} format="relative_short" />
        <Text type="supporting" color="secondary">
          · Quotes are checked against the pull request; the rest is the
          writer’s reading of it.
        </Text>
      </HStack>
    </VStack>
  );
}

function Chapter({ pr, onBack }: { pr: History; onBack: () => void }) {
  const n = pr.narrative;
  const writeUpsOn = useOrbit(
    (state) => state.snapshot.settings.writeUps?.enabled ?? false,
  );
  const plain = (t: Trajectory) =>
    pr.writeup?.findingNotes.find((f) => f.findingId === t.findingId)?.plain;
  const sections: [string, string | null | undefined][] = [
    ["Before", n.before],
    ["After", n.after],
    ["How", n.how],
    ["Decisions", n.decisions],
  ];
  const told = sections.filter(([, text]) => text?.trim());
  const columns: TableColumn<FindingRow>[] = [
    {
      key: "finding",
      header: "Finding",
      width: proportional(2),
      renderCell: ({ t }) => (
        <VStack gap={0}>
          <Text>{t.shortId ? `${t.shortId} · ${t.title}` : t.title}</Text>
          {plain(t) && <Text color="secondary">{plain(t)}</Text>}
          {t.category && (
            <Text type="supporting">{categoryLabels[t.category]}</Text>
          )}
        </VStack>
      ),
    },
    {
      key: "priority",
      header: "Priority",
      width: pixel(80),
      renderCell: ({ t }) => <PriorityToken priority={t.priority} />,
    },
    {
      key: "opened",
      header: "Found in",
      width: pixel(88),
      renderCell: ({ t }) => <Text>{`Round ${t.openedRound}`}</Text>,
    },
    {
      key: "status",
      header: "Where it ended up",
      width: pixel(168),
      renderCell: ({ t }) => <StatusToken status={t.finalStatus} />,
    },
    {
      key: "said",
      header: "The builder said",
      width: proportional(2),
      renderCell: ({ t }) => (
        <Text color="secondary">{said(t.claim) || "Nothing yet"}</Text>
      ),
    },
  ];
  return (
    <VStack gap={6}>
      <HStack>
        <Button
          label="All pull requests"
          size="sm"
          variant="ghost"
          icon={<ArrowLeft />}
          onClick={onBack}
        />
      </HStack>
      <VStack gap={2}>
        <HStack gap={2} vAlign="center" wrap="wrap">
          <StateToken pr={pr} />
          {n.epic != null && (
            <Token
              label={`Part of #${n.epic}${n.stage ? ` · stage ${n.stage}` : ""}`}
              size="sm"
            />
          )}
        </HStack>
        <Heading level={2}>{pr.title}</Heading>
        <HStack gap={1} vAlign="center" wrap="wrap">
          <Text color="secondary">
            {`${pr.repo} #${pr.number} · ${pr.author} · opened`}
          </Text>
          <Timestamp value={pr.createdAt} format="relative_short" />
          {pr.mergedAt && (
            <>
              <Text color="secondary">· merged</Text>
              <Timestamp value={pr.mergedAt} format="relative_short" />
            </>
          )}
        </HStack>
        <HStack>
          <StackItem>
            <Button
              label="Open on GitHub"
              size="sm"
              variant="ghost"
              icon={<ArrowUpRight />}
              onClick={() => openLink(pr.url)}
            />
          </StackItem>
        </HStack>
      </VStack>
      {pr.writeup ? (
        <InPlainWords w={pr.writeup} />
      ) : (
        !writeUpsOn &&
        pr.state !== "open" && (
          <Text type="supporting" color="secondary">
            Turn on write-ups in Settings to have each merged or closed pull
            request summed up in plain words.
          </Text>
        )
      )}
      <VStack gap={3}>
        <Heading level={3}>What changed</Heading>
        {told.length ? (
          told.map(([label, text]) => (
            <VStack key={label} gap={1}>
              <Text weight="medium">{label}</Text>
              <SafeMarkdown>{text!}</SafeMarkdown>
            </VStack>
          ))
        ) : n.lead ? (
          <SafeMarkdown>{n.lead}</SafeMarkdown>
        ) : (
          <Text color="secondary">The description doesn’t say.</Text>
        )}
      </VStack>
      {pr.flags.length > 0 && (
        <List hasDividers header={<Heading level={3}>Worth a look</Heading>}>
          {pr.flags.map((flag, i) => (
            <ListItem
              key={`${flag.kind}-${flag.findingId ?? i}`}
              label={flagLabels[flag.kind]}
              startContent={<FlagDot attention={flag.attention} />}
              description={<Text>{flag.text}</Text>}
            />
          ))}
        </List>
      )}
      <List hasDividers header={<Heading level={3}>Review rounds</Heading>}>
        {pr.rounds.length === 0 && (
          <Text color="secondary">
            No review Orbit could read was posted on this pull request.
          </Text>
        )}
        {pr.rounds.map((round) => (
          <RoundItem
            key={`${round.commentId}-${round.round}-${round.createdAt}`}
            round={round}
          />
        ))}
      </List>
      {pr.trajectories.length > 0 && (
        <VStack gap={2}>
          <Heading level={3}>Findings</Heading>
          <Table
            data={pr.trajectories.map((t) => ({ id: t.findingId, t }))}
            columns={columns}
            idKey="id"
            density="compact"
          />
        </VStack>
      )}
      <MetadataList>
        <MetadataListItem label="Size">
          {`${plural(pr.changedFiles, "file")} · +${pr.additions} −${pr.deletions}`}
        </MetadataListItem>
        <MetadataListItem label="Branch">
          <Text type="code">{`${pr.headRef} → ${pr.baseRef}`}</Text>
        </MetadataListItem>
        {pr.metrics.minutesToClean != null && (
          <MetadataListItem label="Time to a clean round">
            {duration(pr.metrics.minutesToClean)}
          </MetadataListItem>
        )}
        {pr.metrics.mergedByLoop != null && (
          <MetadataListItem label="Merged by the review loop">
            {pr.metrics.mergedByLoop
              ? "Probably: it merged soon after a clean round"
              : "No"}
          </MetadataListItem>
        )}
      </MetadataList>
      {n.checks?.trim() && (
        <VStack gap={1}>
          <Heading level={3}>Checks the builder ran</Heading>
          <SafeMarkdown>{n.checks}</SafeMarkdown>
        </VStack>
      )}
    </VStack>
  );
}

/** The story of every pull request in the connected repositories, read
 * from GitHub without a model: what each set out to do, how its reviews
 * went, what the builder pushed back on, and what is worth a second look. */
export function DecisionsPage() {
  const { action } = useOrbit();
  const version = useOrbit((state) => state.snapshot.ledger?.version ?? 0);
  const refreshing = useOrbit(
    (state) => state.snapshot.ledger?.refreshing ?? false,
  );
  const repos = useOrbit((state) => state.snapshot.settings.repositories);
  const reposKey = repos.map((r) => r.name.toLowerCase()).join(",");
  const [data, setData] = useState<Decisions | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<View>("story");
  const [selected, setSelected] = useState<string | null>(null);
  const [repo, setRepo] = useState("all");
  const [search, setSearch] = useState("");
  useEffect(() => {
    let current = true;
    void action<Decisions>("get_decisions", {}, { quiet: true }).then(
      (result) => {
        if (!current) return;
        if (result.ok && result.data) {
          setData(result.data);
          setError(null);
        } else if (!result.ok) setError(result.error);
      },
    );
    return () => {
      current = false;
    };
  }, [action, version, reposKey]);
  const all = data?.prs ?? [];
  const inRepo = all.filter((h) => repo === "all" || sameRepo(h.repo, repo));
  const prs = searchHistories(inRepo, search);
  const chosen = all.find((h) => historyKey(h) === selected);
  if (chosen)
    return (
      <VStack gap={6} padding={6} maxWidth={960} className="content-page">
        <Chapter pr={chosen} onBack={() => setSelected(null)} />
      </VStack>
    );
  const open = (pr: History) => setSelected(historyKey(pr));
  return (
    <VStack gap={6} padding={6} maxWidth={1100} className="content-page">
      <VStack gap={2}>
        <HStack gap={3} vAlign="center" hAlign="between" wrap="wrap">
          <Heading level={2}>Decisions</Heading>
          {live && (
            <Button
              label="Read again"
              size="sm"
              variant="ghost"
              icon={<RefreshCw />}
              isLoading={refreshing}
              isDisabled={refreshing}
              onClick={() => void action("refresh_decisions")}
            />
          )}
        </HStack>
        <Text color="secondary">
          The story of each pull request in your connected repositories: what it
          set out to do, what each review round found, what was fixed or pushed
          back on, and what is worth a second look. Orbit reads it from GitHub;
          no model is involved.
        </Text>
        <ReadStatus />
        {error && (
          <Banner
            status="error"
            title="Couldn't load the decisions"
            description={error}
            collapsible={false}
          />
        )}
      </VStack>
      {!data && !error ? (
        <Spinner label="Loading decisions…" />
      ) : all.length === 0 ? (
        <EmptyState
          icon={<Scale />}
          title="No pull requests read yet"
          description={
            repos.length
              ? "Orbit reads each connected repository's recent pull requests from GitHub in the background. They will show here."
              : "Connect a repository and Orbit will read its pull requests from GitHub."
          }
        />
      ) : (
        <>
          <HStack gap={2} wrap="wrap" vAlign="center">
            <StackItem size="fill">
              <TextInput
                width="100%"
                label="Search pull requests"
                isLabelHidden
                placeholder="Search pull requests…"
                value={search}
                onChange={setSearch}
                startIcon={<Search />}
              />
            </StackItem>
            <Selector
              label="Repository"
              isLabelHidden
              size="sm"
              value={repo}
              onChange={setRepo}
              options={[
                { value: "all", label: "All repos" },
                ...[...new Set(all.map((h) => h.repo))]
                  .sort()
                  .map((r) => ({ value: r, label: r })),
              ]}
            />
          </HStack>
          <WorthALook rows={worthALook(prs)} onOpen={open} />
          <VStack gap={4}>
            <TabList
              role="tablist"
              aria-label="Decisions views"
              value={view}
              onChange={(v) => setView(v as View)}
              hasDivider
            >
              <Tab
                id="decisions-tab-story"
                value="story"
                label="Story"
                panelId="decisions-panel"
              />
              <Tab
                id="decisions-tab-pushback"
                value="pushback"
                label="Pushback"
                panelId="decisions-panel"
              />
              <Tab
                id="decisions-tab-health"
                value="health"
                label="Review health"
                panelId="decisions-panel"
              />
            </TabList>
            <VStack
              gap={4}
              id="decisions-panel"
              role="tabpanel"
              aria-labelledby={`decisions-tab-${view}`}
            >
              {view === "story" ? (
                <Story prs={prs} onOpen={open} />
              ) : view === "pushback" ? (
                <Pushback prs={prs} onOpen={open} />
              ) : (
                <ReviewHealth prs={prs} />
              )}
            </VStack>
          </VStack>
          {data && data.problems.length > 0 && (
            <Text type="supporting">
              {`${plural(data.problems.length, "stored pull request")} couldn't be read and will be fetched again.`}
            </Text>
          )}
        </>
      )}
    </VStack>
  );
}
