import {
  Badge,
  Button,
  EmptyState,
  Grid,
  Heading,
  HStack,
  List,
  ListItem,
  StatusDot,
  Text,
  Timestamp,
  VStack,
} from "@astryxdesign/core";
import { Activity as ActivityIcon, GitPullRequest, Square } from "lucide-react";
import { useOrbit } from "./store";
import {
  activeStatuses,
  attentionLabels,
  reportSelectionLabel,
  type Attention,
  reviewDisplayStatus,
  type PullRequest,
  type Review,
} from "./types";
import { ReviewActions } from "./ReviewActions";
import { ReviewStatus } from "./ReviewStatus";
import { recentTimestamp } from "./timestamps";
export function Activity({
  onChoose,
  onRepositories,
  onSettings,
  onApproval,
}: {
  onChoose: (p: PullRequest, r?: Review) => void;
  onRepositories: () => void;
  onSettings: () => void;
  onApproval: (reviewId: string, id: string | number) => void;
}) {
  const { snapshot: s, action } = useOrbit();
  const knownPrs = [...s.prs, ...(s.historyPrs ?? [])];
  const attention = s.attention ?? [];
  /** Where each item is resolved: its review, its PR, or a settings page. */
  const open = (a: Attention) => {
    if (a.kind === "sync_failing" && a.repo) return onRepositories;
    if (a.kind === "autopilot_paused" || a.kind === "sync_failing")
      return onSettings;
    const p = knownPrs.find((p) => p.id === a.prId);
    const r = s.reviews.find((r) => r.id === a.reviewId);
    if (a.kind === "approval" && r && a.approvalId != null) {
      const id = a.approvalId;
      return () => {
        if (p) onChoose(p, r);
        onApproval(r.id, id);
      };
    }
    if (!p) return undefined;
    return () => onChoose(p, r);
  };
  const metrics: [
    [string, number],
    [string, number],
    [string, number],
    [string, number],
  ] = [
    [
      "In progress",
      s.reviews.filter((r) => activeStatuses.includes(r.status)).length,
    ],
    [
      "Ready to publish",
      s.reviews.filter((r) => reviewDisplayStatus(s, r) === "ready").length,
    ],
    ["Published", s.reviews.filter((r) => r.status === "posted").length],
    ["Total reviews", s.reviews.length],
  ];
  return (
    <VStack gap={6} padding={6} maxWidth={960} className="content-page">
      <VStack gap={2}>
        <Heading level={2}>Good reviews, in motion.</Heading>
        <Text color="secondary">
          Every run, every finding, every conversation. All in one place.
        </Text>
      </VStack>
      {attention.length > 0 && (
        <List hasDividers header={<Heading level={3}>Needs you</Heading>}>
          {attention.map((a) => {
            const p = knownPrs.find((p) => p.id === a.prId);
            return (
              <ListItem
                key={a.key}
                label={
                  <Text weight="medium" maxLines={2}>
                    {p?.title ?? a.repo ?? attentionLabels[a.kind]}
                  </Text>
                }
                onClick={open(a)}
                startContent={
                  <StatusDot
                    variant="warning"
                    label={attentionLabels[a.kind]}
                  />
                }
                description={
                  <VStack gap={1}>
                    <Text color="secondary">
                      {[attentionLabels[a.kind], a.prId]
                        .filter(Boolean)
                        .join(" · ")}
                    </Text>
                    <Text maxLines={3}>{a.reason}</Text>
                  </VStack>
                }
              />
            );
          })}
        </List>
      )}
      <Grid columns={{ minWidth: 140 }} gap={4}>
        {metrics.map(([label, value]) => (
          <VStack gap={2} key={label}>
            <Text color="secondary">{label}</Text>
            <HStack>
              <Badge label={value} />
            </HStack>
          </VStack>
        ))}
      </Grid>
      <List
        hasDividers
        density="spacious"
        header={<Heading level={3}>Review history</Heading>}
      >
        {s.reviews.map((r, index) => {
          const p = knownPrs.find((p) => p.id === r.prId);
          return (
            <ListItem
              key={r.id}
              label={
                <Text weight="medium" maxLines={3}>
                  {p?.title ?? r.prId}
                </Text>
              }
              onClick={p ? () => onChoose(p, r) : undefined}
              startContent={<GitPullRequest />}
              endContent={
                <VStack gap={2}>
                  <ReviewActions review={r} />
                  {activeStatuses.includes(r.status) && (
                    <Button
                      label={`Stop review ${r.prId}`}
                      size="sm"
                      icon={<Square />}
                      onClick={() =>
                        void action("cancel_review", { reviewId: r.id })
                      }
                    />
                  )}
                </VStack>
              }
              description={
                <VStack gap={3} paddingBlock={2}>
                  <Text color="secondary">
                    {r.prId} · {reportSelectionLabel(r)} ·{" "}
                    {r.automatic ? "Automatic" : "Manual"}
                  </Text>
                  <HStack gap={3} vAlign="center" wrap="wrap">
                    <ReviewStatus status={reviewDisplayStatus(s, r)} />
                    <Timestamp
                      value={r.updatedAt}
                      {...recentTimestamp(index)}
                    />
                  </HStack>
                </VStack>
              }
            />
          );
        })}
      </List>
      {!s.reviews.length && (
        <EmptyState
          icon={<ActivityIcon />}
          title="The story starts with a review"
          description="Start a review from your pull request inbox to see its progress here."
        />
      )}
    </VStack>
  );
}
