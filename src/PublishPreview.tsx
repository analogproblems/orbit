import { Banner, Spinner, Text, Token, VStack } from "@astryxdesign/core";
import {
  Table,
  pixel,
  proportional,
  type TableColumn,
} from "@astryxdesign/core/Table";
import { useEffect, useState } from "react";
import { FindingDetails, priorityColors } from "./Findings";
import { SafeMarkdown } from "./SafeMarkdown";
import { useOrbit } from "./store";
import {
  findingLocation,
  type Finding,
  type PublishPlan,
  type Review,
} from "./types";

interface Row extends Finding {
  placement: string;
}

const columns: TableColumn<Row>[] = [
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
        <Text type="code">{findingLocation(f)}</Text>
        <Text type="supporting">{f.body}</Text>
        <FindingDetails finding={f} />
      </VStack>
    ),
  },
  {
    key: "placement",
    header: "Goes",
    width: proportional(2),
    renderCell: (f) => <Text type="supporting">{f.placement}</Text>,
  },
];

/**
 * What publishing will post. A run with a findings list becomes a review:
 * each finding on its lines when they are in the diff, otherwise in the
 * review text with the reason. An unstructured run is one comment.
 */
export function PublishPreview({ review }: { review: Review }) {
  const action = useOrbit((state) => state.action);
  const [plan, setPlan] = useState<PublishPlan | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(!!review.findings);
  const structured = !!review.findings;
  useEffect(() => {
    if (!structured) return;
    let alive = true;
    void action<PublishPlan | null>(
      "publish_plan",
      { reviewId: review.id },
      { quiet: true },
    ).then((result) => {
      if (!alive) return;
      setLoading(false);
      setPlan(result.ok ? (result.data ?? null) : null);
      setError(result.ok ? null : result.error);
    });
    return () => {
      alive = false;
    };
  }, [action, review.id, structured]);
  if (!structured || (!loading && !plan && !error))
    return <SafeMarkdown>{review.report}</SafeMarkdown>;
  if (loading) return <Spinner label="Checking which lines are in the diff…" />;
  if (!plan)
    return (
      <Banner
        status="warning"
        title="Couldn’t check the diff"
        description={`${error} Orbit checks it again when publishing, so each finding still goes on its lines when it can.`}
      />
    );
  const rows: Row[] = (review.findings ?? []).map((f) => {
    const p = plan.placements.find((p) => p.findingId === f.id);
    return {
      ...f,
      placement: p?.inline
        ? "On its lines"
        : `In the review text: ${p?.reason ?? "not checked"}`,
    };
  });
  const inline = rows.filter((r) => r.placement === "On its lines").length;
  return (
    <VStack gap={3}>
      <Text type="supporting">
        {rows.length
          ? `${inline} of ${rows.length} ${rows.length === 1 ? "finding goes" : "findings go"} on the lines they are about. The rest ${rows.length - inline === 1 ? "goes" : "go"} in the review text.`
          : "No findings. Only the review text is posted."}
      </Text>
      {rows.length > 0 && (
        <Table
          data={rows}
          columns={columns}
          idKey="id"
          density="compact"
          verticalAlign="top"
        />
      )}
      <Text weight="medium">Review text</Text>
      <SafeMarkdown>{plan.body}</SafeMarkdown>
    </VStack>
  );
}
