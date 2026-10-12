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
import { ArrowLeft, ArrowUpRight, Bug, FileText, Search } from "lucide-react";
import { openLink, useOrbit } from "./store";
import {
  categoryLabels,
  findingLocation,
  isBlocking,
  priorStatusLabels,
  shortId,
  type Finding,
  type AttentionHandlers,
  type Category,
} from "./types";
import {
  collectFindings,
  findingCommentUrl,
  findingFileUrl,
  findingStatusColors,
  findingSourceLabels,
  findingStatusLabels,
  type FindingRow,
  type FindingSource,
  type FindingStatus,
} from "./findingIndex";
import { FindingKind, priorityColors } from "./Findings";
import { SafeMarkdown } from "./SafeMarkdown";
import { ScanReportDialog } from "./Scans";

const statuses = Object.keys(findingStatusLabels) as FindingStatus[];
const PAGE = 100;

export interface FindingFilters {
  search: string;
  repo: string;
  /** "all" or a priority, 0 to 3. */
  severity: string;
  /** "all" or a category; older findings have none. */
  category: string;
  status: string;
  /** "all", "open" or "closed": whether the PR is still in the inbox. */
  pr: string;
  /** "all", or where findings came from: "review" or "scan". */
  source: string;
}
export const noFilters: FindingFilters = {
  search: "",
  repo: "all",
  severity: "all",
  category: "all",
  status: "all",
  pr: "all",
  source: "all",
};

export function filterFindings(rows: FindingRow[], f: FindingFilters) {
  const words = f.search.toLowerCase().split(/\s+/).filter(Boolean);
  return rows.filter((row) => {
    if (f.repo !== "all" && row.repo.toLowerCase() !== f.repo.toLowerCase())
      return false;
    if (f.severity !== "all" && row.finding.priority !== Number(f.severity))
      return false;
    if (f.category !== "all" && row.finding.category !== f.category)
      return false;
    if (f.status !== "all" && row.status !== f.status) return false;
    if (f.source !== "all" && row.source !== f.source) return false;
    // A scan's finding has no pull request, open or closed.
    if (f.pr === "open" && !row.prOpen) return false;
    if (f.pr === "closed" && (row.prOpen || row.source !== "review"))
      return false;
    const g = row.finding;
    const text = [
      shortId(g) ?? "",
      g.title,
      g.body,
      g.path ?? "",
      ...(g.otherLocations ?? []).map((l) => l.path ?? ""),
      g.failureScenario ?? "",
      g.evidence ?? "",
      g.suggestedFix ?? "",
      g.doneWhen ?? "",
      g.reviewerNotes ?? "",
      row.prId ?? "",
      row.pr?.title ?? "",
    ]
      .join(" ")
      .toLowerCase();
    return words.every((w) => text.includes(w));
  });
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
          label="Category"
          isLabelHidden
          size="sm"
          value={filters.category}
          onChange={set("category")}
          options={[
            { value: "all", label: "All categories" },
            ...(Object.keys(categoryLabels) as Category[]).map((c) => ({
              value: c,
              label: categoryLabels[c],
            })),
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
          label="Source"
          isLabelHidden
          size="sm"
          value={filters.source}
          onChange={set("source")}
          options={[
            { value: "all", label: "Reviews and scans" },
            ...(Object.keys(findingSourceLabels) as FindingSource[]).map(
              (s) => ({ value: s, label: findingSourceLabels[s] }),
            ),
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
  const [report, setReport] = useState(false);
  const openReview = (id: string) => {
    const review = s.reviews.find((r) => r.id === id);
    if (row.pr && review) onChoose(row.pr, review);
  };
  const commit = row.commit?.slice(0, 7);
  // Older findings have none of these; their body holds it all.
  const details: [string, string | null | undefined][] = [
    ["Breaks when", f.failureScenario],
    ["Evidence", f.evidence],
    ["Suggested fix", f.suggestedFix],
    ["Done when", f.doneWhen],
    ["Reviewer's notes", f.reviewerNotes],
  ];
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
          {row.review && isBlocking(f, row.review) && (
            <Token label="Blocking" color="red" size="sm" />
          )}
          <StatusToken status={row.status} />
        </HStack>
        <Heading level={2}>{withShortId(f)}</Heading>
        <Text color="secondary">
          {row.scan
            ? `Security scan of ${row.repo}${commit ? ` at ${commit}` : ""}`
            : row.pr
              ? `${row.pr.title} · ${row.prId}`
              : row.prId}
        </Text>
      </VStack>
      <FindingKind finding={f} />
      <SafeMarkdown>{f.body}</SafeMarkdown>
      {details
        .filter(([, text]) => text?.trim())
        .map(([label, text]) => (
          <VStack key={label} gap={1}>
            <Heading level={3}>{label}</Heading>
            <SafeMarkdown>{text!}</SafeMarkdown>
          </VStack>
        ))}
      <MetadataList>
        <MetadataListItem
          label={f.otherLocations?.length ? "Locations" : "Location"}
        >
          <VStack gap={1}>
            {[f, ...(f.otherLocations ?? [])].map((l) => (
              <Text key={findingLocation(l)} type="code">
                {findingLocation(l)}
              </Text>
            ))}
          </VStack>
        </MetadataListItem>
        {commit && (
          <MetadataListItem
            label={row.scan ? "Scanned commit" : "Reviewed commit"}
          >
            <Text type="code">{commit}</Text>
          </MetadataListItem>
        )}
        <MetadataListItem label="Found">
          <HStack gap={1} vAlign="center">
            <Text>
              {row.scan
                ? `${row.scan.mode === "deep" ? "Deep" : "Standard"} security scan ·`
                : `Round ${row.round} ·`}
            </Text>
            <Timestamp value={row.foundAt} format="relative_short" />
          </HStack>
        </MetadataListItem>
        <MetadataListItem label="Confidence">
          {`${Math.round(f.confidence * 100)}%`}
        </MetadataListItem>
        {f.confirmed != null && (
          <MetadataListItem label="Verified">
            {f.confirmed
              ? "Yes, by reading or running the code"
              : "No, it is likely but unverified"}
          </MetadataListItem>
        )}
        {row.source === "review" && (
          <MetadataListItem label="Pull request">
            {row.prOpen ? "In your inbox" : "Left the inbox"}
          </MetadataListItem>
        )}
      </MetadataList>
      <HStack gap={2} wrap="wrap">
        {row.pr && row.review && (
          <Button
            label="Open review chat"
            size="sm"
            onClick={() => openReview(row.review!.id)}
          />
        )}
        {row.scan?.hasReport && (
          <Button
            label="Read scan report"
            size="sm"
            icon={<FileText />}
            onClick={() => setReport(true)}
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
      {row.source === "review" && (
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
      )}
      <ScanReportDialog
        scan={report ? (row.scan ?? null) : null}
        onClose={() => setReport(false)}
      />
    </VStack>
  );
}

/** A finding's title after its short ID, as the PR's comments name it. */
function withShortId(f: Finding) {
  const id = shortId(f);
  return id ? `${id} · ${f.title}` : f.title;
}

/** Where a finding came from, with its category for a review's finding. A
 * scan's findings are all about security. */
function origin(r: FindingRow) {
  if (r.scan) return `Security scan · ${r.repo} ·`;
  const category = r.finding.category
    ? `${categoryLabels[r.finding.category]} · `
    : "";
  return `${category}${r.prId} · Round ${r.round} ·`;
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
          Every finding from Orbit’s pull request reviews and security scans,
          across repositories. A review’s finding stays open until a later round
          marks it fixed or declined.
        </Text>
      </VStack>
      {rows.length === 0 ? (
        <EmptyState
          icon={<Bug />}
          title="No findings yet"
          description="Findings from finished reviews and security scans will be collected here."
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
                  label={withShortId(r.finding)}
                  onClick={() => onSelect(r.finding.id)}
                  startContent={<SeverityToken priority={r.finding.priority} />}
                  endContent={<StatusToken status={r.status} />}
                  description={
                    <HStack gap={1} vAlign="center" wrap="wrap">
                      <Text color="secondary">{origin(r)}</Text>
                      <Timestamp value={r.foundAt} format="relative_short" />
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
