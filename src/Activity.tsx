import {
  Badge,
  Button,
  EmptyState,
  Grid,
  Heading,
  HStack,
  List,
  ListItem,
  Text,
  Timestamp,
  VStack,
} from "@astryxdesign/core";
import { Activity as ActivityIcon, GitPullRequest, Square } from "lucide-react";
import { useOrbit } from "./store";
import {
  activeStatuses,
  reportSelectionLabel,
  reviewDisplayStatus,
  type PullRequest,
  type Review,
} from "./types";
import { ReviewActions } from "./ReviewActions";
import { ReviewStatus } from "./ReviewStatus";
export function Activity({
  onChoose,
}: {
  onChoose: (p: PullRequest, r: Review) => void;
}) {
  const { snapshot: s, action } = useOrbit();
  const knownPrs = [...s.prs, ...(s.historyPrs ?? [])];
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
        {s.reviews.map((r) => {
          const p = knownPrs.find((p) => p.id === r.prId);
          return (
            <ListItem
              key={r.id}
              label={
                <Text weight="medium" maxLines={3}>
                  {p?.title ?? r.prId}
                </Text>
              }
              isDisabled={!p}
              onClick={() => p && onChoose(p, r)}
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
                      format="relative_short"
                      isLive
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
