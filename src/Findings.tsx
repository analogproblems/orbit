import {
  Banner,
  Button,
  HStack,
  Text,
  Token,
  VStack,
} from "@astryxdesign/core";
import {
  Table,
  pixel,
  proportional,
  type TableColumn,
} from "@astryxdesign/core/Table";
import { useMemo } from "react";
import { useOrbit } from "./store";
import {
  collectFindings,
  findingStatusColors,
  findingStatusLabels,
  type FindingStatus,
} from "./findingIndex";
import {
  categoryLabels,
  findingLocation,
  findingReviewId,
  notChecked,
  priorStatusLabels,
  providerLabels,
  type Finding,
  type PriorStatus,
  type Review,
} from "./types";

export const priorityColors = ["red", "orange", "yellow", "gray"] as const;

const columns = (
  statusOf: (f: Finding) => FindingStatus,
): TableColumn<Finding>[] => [
  {
    key: "priority",
    header: "Severity",
    width: pixel(88),
    renderCell: (f) => (
      <Token
        label={`P${f.priority}`}
        color={priorityColors[f.priority] ?? "gray"}
        size="sm"
      />
    ),
  },
  {
    key: "title",
    header: "Finding",
    width: proportional(3),
    renderCell: (f) => (
      <VStack gap={1}>
        <Text weight="medium">{f.title}</Text>
        <Text type="supporting">{f.body}</Text>
        {f.failureScenario && (
          <Text type="supporting">Breaks when: {f.failureScenario}</Text>
        )}
        <FindingKind finding={f} />
      </VStack>
    ),
  },
  {
    key: "path",
    header: "Location",
    width: proportional(2),
    renderCell: (f) => (
      <VStack gap={1}>
        <Text type="code">{findingLocation(f)}</Text>
        {f.otherLocations?.map((l) => (
          <Text key={findingLocation(l)} type="code" color="secondary">
            {findingLocation(l)}
          </Text>
        ))}
      </VStack>
    ),
  },
  {
    key: "status",
    header: "Status",
    width: pixel(150),
    // Open until a later round marks it fixed or declined.
    renderCell: (f) => (
      <Token
        label={findingStatusLabels[statusOf(f)]}
        color={findingStatusColors[statusOf(f)]}
        size="sm"
      />
    ),
  },
];

/** A finding's category, and whether it is only likely. Absent from older
 * findings. */
export function FindingKind({ finding: f }: { finding: Finding }) {
  if (!f.category && f.confirmed !== false) return null;
  return (
    <HStack gap={1} wrap="wrap">
      {f.category && (
        <Token label={categoryLabels[f.category]} color="gray" size="sm" />
      )}
      {f.confirmed === false && (
        <Token label="Likely, not verified" color="yellow" size="sm" />
      )}
    </HStack>
  );
}

/** Everything a finding says beyond its title and problem, one line each,
 * as it will be published. Empty for older findings. */
export function FindingDetails({ finding: f }: { finding: Finding }) {
  const lines: [string, string | null | undefined][] = [
    ["Breaks when", f.failureScenario],
    ["Evidence", f.evidence],
    ["Suggested fix", f.suggestedFix],
    ["Done when", f.doneWhen],
    ["Also at", f.otherLocations?.map(findingLocation).join(", ")],
    ["Reviewer's notes", f.reviewerNotes],
  ];
  return (
    <>
      <FindingKind finding={f} />
      {lines
        .filter(([, text]) => text?.trim())
        .map(([label, text]) => (
          <Text key={label} type="supporting">
            <Text type="supporting" weight="semibold">
              {label}:
            </Text>{" "}
            {text}
          </Text>
        ))}
    </>
  );
}

const priorColors: Record<PriorStatus, "green" | "orange" | "gray" | "red"> = {
  fixed: "green",
  still_open: "orange",
  declined_accepted: "gray",
  declined_disputed: "red",
};

interface EarlierFinding extends Finding {
  status: PriorStatus | null;
  note: string;
}

/** What a re-review decided about the findings of earlier rounds. */
function SinceLastRound({
  review,
  onOpenReview,
}: {
  review: Review;
  onOpenReview?: (id: string) => void;
}) {
  const reviews = useOrbit((state) => state.snapshot.reviews);
  const round = review.previousRound;
  if (!round || round.findings.length === 0) return null;
  const rows: EarlierFinding[] = round.findings.map((f) => {
    const prior = review.priorFindings?.find((p) => p.id === f.id);
    return { ...f, status: prior?.status ?? null, note: prior?.note ?? "" };
  });
  const counts = (Object.keys(priorStatusLabels) as PriorStatus[])
    .map((status) => {
      const n = rows.filter((r) => r.status === status).length;
      return n ? `${n} ${priorStatusLabels[status]}` : null;
    })
    .concat(
      rows.some((r) => !r.status)
        ? `${rows.filter((r) => !r.status).length} not reported`
        : null,
    )
    .filter(Boolean);
  const columns: TableColumn<EarlierFinding>[] = [
    {
      key: "status",
      header: "Now",
      width: pixel(160),
      renderCell: (f) =>
        f.status ? (
          <Token
            label={priorStatusLabels[f.status]}
            color={priorColors[f.status]}
            size="sm"
          />
        ) : (
          <Text type="supporting">Not reported</Text>
        ),
    },
    {
      key: "title",
      header: "Earlier finding",
      width: proportional(3),
      renderCell: (f) => (
        <VStack gap={1}>
          <Text weight="medium">
            P{f.priority} · {f.title}
          </Text>
          {f.note && <Text type="supporting">{f.note}</Text>}
          <Text type="code">{findingLocation(f)}</Text>
        </VStack>
      ),
    },
    {
      key: "id",
      header: "",
      width: pixel(120),
      renderCell: (f) => {
        const id = findingReviewId(f);
        return onOpenReview && reviews.some((r) => r.id === id) ? (
          <Button
            label="Open review"
            variant="ghost"
            size="sm"
            onClick={() => onOpenReview(id)}
          />
        ) : null;
      },
    },
  ];
  return (
    <VStack gap={2}>
      <Text weight="medium">Since the last round</Text>
      <Text type="supporting">{counts.join(", ")}</Text>
      <Table
        data={rows}
        columns={columns}
        idKey="id"
        density="compact"
        verticalAlign="top"
      />
    </VStack>
  );
}

/** Orbit's verdict when it has one, else the model's (older reviews). */
function verdictLine(review: Review, count: number) {
  const findings = `${count} ${count === 1 ? "finding" : "findings"}`;
  if (review.orbitVerdict === "clean")
    return count
      ? `Verdict: clean · ${findings}, none blocking`
      : "Verdict: clean";
  if (review.orbitVerdict)
    return `Verdict: changes requested · ${review.blocking ?? 0} blocking · ${findings}`;
  return review.verdict === "clean"
    ? "Verdict: clean"
    : `Verdict: changes requested · ${findings}`;
}

/** The structured findings of a finished review, or why there are none. */
export function Findings({
  review,
  onOpenReview,
}: {
  review: Review;
  /** Shows an earlier review whose finding this re-review checked. */
  onOpenReview?: (id: string) => void;
}) {
  const s = useOrbit((state) => state.snapshot);
  const statuses = useMemo(
    () => new Map(collectFindings(s).map((r) => [r.finding.id, r.status])),
    [s],
  );
  if (
    review.extracting ||
    !["ready", "publishing", "posted"].includes(review.status)
  )
    return null;
  // Shown whether or not the findings were extracted.
  const declined = review.declinedApprovals?.map((note) => (
    <Text key={note} type="supporting">
      {note}
    </Text>
  ));
  if (!review.findings)
    return (
      <VStack gap={2}>
        <Banner
          status="info"
          title="Unstructured run"
          description="Orbit could not save these findings as a list. The report above is complete."
        />
        {declined}
      </VStack>
    );
  return (
    <VStack gap={2}>
      <Text weight="medium">{verdictLine(review, review.findings.length)}</Text>
      {review.orbitVerdict &&
        review.verdict &&
        review.orbitVerdict !== review.verdict && (
          <Text type="supporting">
            {providerLabels[review.provider ?? "codex"]}’s own verdict:{" "}
            {review.verdict === "clean" ? "clean" : "changes requested"}.
          </Text>
        )}
      {review.summary && <Text type="supporting">{review.summary}</Text>}
      {review.findings.length > 0 && (
        <Table
          data={review.findings}
          columns={columns((f) => statuses.get(f.id) ?? "open")}
          idKey="id"
          density="compact"
          verticalAlign="top"
        />
      )}
      {notChecked(review).length > 0 && (
        <Text type="supporting">
          Not checked: {notChecked(review).join("; ")}
        </Text>
      )}
      {declined}
      <SinceLastRound review={review} onOpenReview={onOpenReview} />
    </VStack>
  );
}
