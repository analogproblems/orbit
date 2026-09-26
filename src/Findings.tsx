import { Banner, Text, Token, VStack } from "@astryxdesign/core";
import {
  Table,
  pixel,
  proportional,
  type TableColumn,
} from "@astryxdesign/core/Table";
import { findingLocation, type Finding, type Review } from "./types";

const priorityColors = ["red", "orange", "yellow", "gray"] as const;

const columns: TableColumn<Finding>[] = [
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
      </VStack>
    ),
  },
  {
    key: "path",
    header: "Location",
    width: proportional(2),
    renderCell: (f) => <Text type="code">{findingLocation(f)}</Text>,
  },
  {
    key: "status",
    header: "Status",
    width: pixel(88),
    // Every finding is open until later rounds track what was decided.
    renderCell: () => <Text type="supporting">Open</Text>,
  },
];

/** The structured findings of a finished review, or why there are none. */
export function Findings({ review }: { review: Review }) {
  if (
    review.extracting ||
    !["ready", "publishing", "posted"].includes(review.status)
  )
    return null;
  if (!review.findings)
    return (
      <Banner
        status="info"
        title="Unstructured run"
        description="Orbit could not save these findings as a list. The report above is complete."
      />
    );
  return (
    <VStack gap={2}>
      <Text weight="medium">
        {review.verdict === "clean"
          ? "Verdict: clean"
          : `Verdict: changes requested · ${review.findings.length} ${
              review.findings.length === 1 ? "finding" : "findings"
            }`}
      </Text>
      {review.summary && <Text type="supporting">{review.summary}</Text>}
      {review.findings.length > 0 && (
        <Table
          data={review.findings}
          columns={columns}
          idKey="id"
          density="compact"
          verticalAlign="top"
        />
      )}
      {review.testsNotRun && review.testsNotRun.length > 0 && (
        <Text type="supporting">Not run: {review.testsNotRun.join("; ")}</Text>
      )}
    </VStack>
  );
}
