import { useMemo, useState } from "react";
import {
  Button,
  EmptyState,
  Heading,
  HStack,
  List,
  ListItem,
  MetadataList,
  MetadataListItem,
  Selector,
  StackItem,
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
import { ArrowLeft, ArrowUpRight, Bug, Search } from "lucide-react";
import { desktop, useOrbit } from "./store";
import {
  findingLocation,
  priorStatusLabels,
  type AttentionHandlers,
} from "./types";
import {
  collectFindings,
  findingCommentUrl,
  findingFileUrl,
  findingStatusColors,
  findingStatusLabels,
  type FindingRow,
  type FindingStatus,
} from "./findingIndex";
import { priorityColors } from "./Findings";
import { SafeMarkdown } from "./SafeMarkdown";

const statuses = Object.keys(findingStatusLabels) as FindingStatus[];
const PAGE = 100;

export interface FindingFilters {
  search: string;
  repo: string;
  /** "all" or a priority, 0 to 3. */
  severity: string;
  status: string;
  /** "all", "open" or "closed": whether the PR is still in the inbox. */
  pr: string;
}
export const noFilters: FindingFilters = {
  search: "",
  repo: "all",
  severity: "all",
  status: "all",
  pr: "all",
};

export function filterFindings(rows: FindingRow[], f: FindingFilters) {
  const words = f.search.toLowerCase().split(/\s+/).filter(Boolean);
  return rows.filter((row) => {
    if (f.repo !== "all" && row.repo.toLowerCase() !== f.repo.toLowerCase())
      return false;
    if (f.severity !== "all" && row.finding.priority !== Number(f.severity))
      return false;
    if (f.status !== "all" && row.status !== f.status) return false;
    if (f.pr === "open" && !row.prOpen) return false;
    if (f.pr === "closed" && row.prOpen) return false;
    const text = [
      row.finding.title,
      row.finding.body,
      row.finding.path ?? "",
      row.prId,
      row.pr?.title ?? "",
    ]
      .join(" ")
      .toLowerCase();
    return words.every((w) => text.includes(w));
  });
}

function openLink(url: string) {
  if (desktop) void useOrbit.getState().action("open_github", { url });
  else window.open(url, "_blank", "noopener,noreferrer");
}

function SeverityToken({ priority }: { priority: number }) {
  return (
    <Token
      label={`P${priority}`}
      color={priorityColors[priority] ?? "gray"}
      size="sm"
    />
  );
}
function StatusToken({ status }: { status: FindingStatus }) {
  return (
    <Token
      label={findingStatusLabels[status]}
      color={findingStatusColors[status]}
      size="sm"
    />
  );
}

interface OverviewRow extends Record<string, unknown> {
  priority: number;
  counts: Record<FindingStatus, number>;
  total: number;
}
/** Counts by severity and status, for the filtered findings. */
function Overview({ rows }: { rows: FindingRow[] }) {
  const data: OverviewRow[] = [0, 1, 2, 3].map((priority) => {
    const mine = rows.filter((r) => r.finding.priority === priority);
    const counts = Object.fromEntries(
      statuses.map((s) => [s, mine.filter((r) => r.status === s).length]),
    ) as Record<FindingStatus, number>;
    return { priority, counts, total: mine.length };
  });
  const columns: TableColumn<OverviewRow>[] = [
    {
      key: "priority",
      header: "Severity",
      width: pixel(88),
      renderCell: (r) => <SeverityToken priority={r.priority} />,
    },
    ...statuses.map((status): TableColumn<OverviewRow> => ({
      key: status,
      header: findingStatusLabels[status],
      width: proportional(1),
      align: "end",
      renderCell: (r) => <Text>{r.counts[status]}</Text>,
    })),
    {
      key: "total",
      header: "Total",
      width: pixel(72),
      align: "end",
      renderCell: (r) => <Text weight="medium">{r.total}</Text>,
    },
  ];
  return (
    <Table data={data} columns={columns} idKey="priority" density="compact" />
  );
}

function Filters({
  rows,
  filters,
  setFilters,
}: {
  rows: FindingRow[];
  filters: FindingFilters;
  setFilters: (f: FindingFilters) => void;
}) {
  const set = (key: keyof FindingFilters) => (value: string) =>
    setFilters({ ...filters, [key]: value });
  const repos = [...new Set(rows.map((r) => r.repo))].sort();
  return (
    <VStack gap={2}>
      <TextInput
        width="100%"
        label="Search findings"
        isLabelHidden
        placeholder="Search findings…"
        value={filters.search}
        onChange={set("search")}
        startIcon={<Search />}
      />
      <HStack gap={2} wrap="wrap">
        <Selector
          label="Repository"
          isLabelHidden
          size="sm"
          value={filters.repo}
          onChange={set("repo")}
          options={[
            { value: "all", label: "All repos" },
            ...repos.map((repo) => ({ value: repo, label: repo })),
          ]}
        />
        <Selector
          label="Severity"
          isLabelHidden
          size="sm"
          value={filters.severity}
          onChange={set("severity")}
          options={[
            { value: "all", label: "All severities" },
            ...[0, 1, 2, 3].map((p) => ({ value: `${p}`, label: `P${p}` })),
          ]}
        />
        <Selector
          label="Status"
          isLabelHidden
          size="sm"
          value={filters.status}
          onChange={set("status")}
          options={[
            { value: "all", label: "All statuses" },
            ...statuses.map((s) => ({
              value: s,
              label: findingStatusLabels[s],
            })),
          ]}
        />
        <Selector
          label="Pull request"
          isLabelHidden
          size="sm"
          value={filters.pr}
          onChange={set("pr")}
          options={[
            { value: "all", label: "Open and closed PRs" },
            { value: "open", label: "Open PRs" },
            { value: "closed", label: "Closed PRs" },
          ]}
        />
      </HStack>
    </VStack>
  );
}

/** A finding's full explanation, where it is, and its history. */
function FindingDetail({
  row,
  onBack,
  onChoose,
}: {
  row: FindingRow;
  onBack: () => void;
  onChoose: AttentionHandlers["onChoose"];
}) {
  const s = useOrbit((state) => state.snapshot);
  const f = row.finding;
  const fileUrl = findingFileUrl(row);
  const commentUrl = findingCommentUrl(row);
  const openReview = (id: string) => {
    const review = s.reviews.find((r) => r.id === id);
    if (row.pr && review) onChoose(row.pr, review);
  };
  return (
    <VStack gap={4}>
      <HStack>
        <Button
          label="All findings"
          size="sm"
          variant="ghost"
          icon={<ArrowLeft />}
          onClick={onBack}
        />
      </HStack>
      <VStack gap={2}>
        <HStack gap={2} vAlign="center" wrap="wrap">
          <SeverityToken priority={f.priority} />
          <StatusToken status={row.status} />
        </HStack>
        <Heading level={2}>{f.title}</Heading>
        <Text color="secondary">
          {row.pr ? `${row.pr.title} · ${row.prId}` : row.prId}
        </Text>
      </VStack>
      <SafeMarkdown>{f.body}</SafeMarkdown>
      <MetadataList>
        <MetadataListItem label="Location">
          <Text type="code">{findingLocation(f)}</Text>
        </MetadataListItem>
        <MetadataListItem label="Reviewed commit">
          <Text type="code">{row.review.headSha.slice(0, 7)}</Text>
        </MetadataListItem>
        <MetadataListItem label="Found">
          <HStack gap={1} vAlign="center">
            <Text>Round {row.round} ·</Text>
            <Timestamp value={row.review.createdAt} format="relative_short" />
          </HStack>
        </MetadataListItem>
        <MetadataListItem label="Confidence">
          {`${Math.round(f.confidence * 100)}%`}
        </MetadataListItem>
        <MetadataListItem label="Pull request">
          {row.prOpen ? "In your inbox" : "Left the inbox"}
        </MetadataListItem>
      </MetadataList>
      <HStack gap={2} wrap="wrap">
        {row.pr && (
          <Button
            label="Open review chat"
            size="sm"
            onClick={() => openReview(row.review.id)}
          />
        )}
        {fileUrl && (
          <Button
            label="View lines on GitHub"
            size="sm"
            variant="ghost"
            icon={<ArrowUpRight />}
            onClick={() => openLink(fileUrl)}
          />
        )}
        {row.pr && (
          <Button
            label="Open pull request"
            size="sm"
            variant="ghost"
            icon={<ArrowUpRight />}
            onClick={() => openLink(row.pr!.url)}
          />
        )}
        {commentUrl && (
          <Button
            label="View published comment"
            size="sm"
            variant="ghost"
            icon={<ArrowUpRight />}
            onClick={() => openLink(commentUrl)}
          />
        )}
      </HStack>
      <List hasDividers header={<Heading level={3}>History</Heading>}>
        {row.history.map((event) => (
          <ListItem
            key={event.reviewId}
            label={`Round ${event.round}: ${
              event.status ? priorStatusLabels[event.status] : "found"
            }`}
            onClick={row.pr ? () => openReview(event.reviewId) : undefined}
            description={
              <VStack gap={1}>
                {event.note && <Text>{event.note}</Text>}
                <Timestamp value={event.at} format="relative_short" />
              </VStack>
            }
          />
        ))}
      </List>
    </VStack>
  );
}

/** Every finding Orbit has collected, across PRs and repositories. */
export function FindingsPage({
  selected,
  onSelect,
  onChoose,
}: Pick<AttentionHandlers, "onChoose"> & {
  /** The finding whose page is open; the app clears it to show the list. */
  selected: string | null;
  onSelect: (id: string | null) => void;
}) {
  const { snapshot: s } = useOrbit();
  const rows = useMemo(() => collectFindings(s), [s]);
  const [filters, setFilters] = useState(noFilters);
  const [shown, setShown] = useState(PAGE);
  const row = rows.find((r) => r.finding.id === selected);
  if (row)
    return (
      <VStack gap={6} padding={6} maxWidth={960} className="content-page">
        <FindingDetail
          row={row}
          onBack={() => onSelect(null)}
          onChoose={onChoose}
        />
      </VStack>
    );
  const filtered = filterFindings(rows, filters);
  return (
    <VStack gap={6} padding={6} maxWidth={1100} className="content-page">
      <VStack gap={2}>
        <Heading level={2}>Findings</Heading>
        <Text color="secondary">
          Every finding from Orbit’s reviews, across pull requests and
          repositories. A finding stays open until a later round marks it fixed
          or declined.
        </Text>
      </VStack>
      {rows.length === 0 ? (
        <EmptyState
          icon={<Bug />}
          title="No findings yet"
          description="Findings from finished reviews will be collected here."
        />
      ) : (
        <>
          <Filters rows={rows} filters={filters} setFilters={setFilters} />
          <Overview rows={filtered} />
          {filtered.length === 0 ? (
            <Text color="secondary">No findings match these filters.</Text>
          ) : (
            <List
              hasDividers
              header={
                <Heading level={3}>
                  {filtered.length}{" "}
                  {filtered.length === 1 ? "finding" : "findings"}
                </Heading>
              }
            >
              {filtered.slice(0, shown).map((r) => (
                <ListItem
                  key={r.finding.id}
                  label={r.finding.title}
                  onClick={() => onSelect(r.finding.id)}
                  startContent={<SeverityToken priority={r.finding.priority} />}
                  endContent={<StatusToken status={r.status} />}
                  description={
                    <HStack gap={1} vAlign="center" wrap="wrap">
                      <Text color="secondary">
                        {r.prId} · Round {r.round} ·
                      </Text>
                      <Timestamp
                        value={r.review.createdAt}
                        format="relative_short"
                      />
                    </HStack>
                  }
                />
              ))}
            </List>
          )}
          {filtered.length > shown && (
            <HStack>
              <StackItem>
                <Button
                  label={`Show ${Math.min(PAGE, filtered.length - shown)} more`}
                  size="sm"
                  onClick={() => setShown(shown + PAGE)}
                />
              </StackItem>
            </HStack>
          )}
        </>
      )}
    </VStack>
  );
}
