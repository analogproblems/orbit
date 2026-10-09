import { lazy, Suspense } from "react";
import {
  Badge,
  Banner,
  Button,
  Card,
  Code,
  Grid,
  Heading,
  HStack,
  IconButton,
  Layout,
  LayoutContent,
  LayoutFooter,
  LayoutHeader,
  Spinner,
  Tab,
  TabList,
  Text,
  Timestamp,
  Token,
  VStack,
} from "@astryxdesign/core";
import { ArrowRight, ArrowUpRight, Play, Square } from "lucide-react";
import { desktop, live, useOrbit } from "./store";
import {
  activeStatuses,
  canPublish,
  canStartReview,
  publishTarget,
  reportSelectionLabel,
  reviewDisplayStatus,
  sameRepo,
  sameRepository,
  trustedSource,
  reviewerSelectionUnavailable,
  repoProvider,
  type PullRequest,
  type Review,
} from "./types";
import { Files } from "./Files";
import { SafeMarkdown } from "./SafeMarkdown";
import { ReviewActions } from "./ReviewActions";
import { ReviewStatus } from "./ReviewStatus";
import { LoopPanel } from "./Loop";
const Chat = lazy(() => import("./Chat").then((m) => ({ default: m.Chat })));
export type DetailTab = "overview" | "chat" | "files";
export function Detail({
  pr,
  review,
  tab,
  setTab,
  busy,
  onStart,
  onPublish,
  onHistory,
  onSettings,
  onRepositories,
}: {
  pr: PullRequest;
  review?: Review;
  tab: DetailTab;
  setTab: (v: DetailTab) => void;
  busy: boolean;
  onStart: () => void;
  onPublish: (r: Review) => void;
  onHistory: (id: string) => void;
  onSettings: () => void;
  onRepositories: () => void;
}) {
  const { snapshot: s, action, updateSettings } = useOrbit();
  const reviewBlocked = reviewerSelectionUnavailable(
    s,
    repoProvider(s, pr.repo),
  );
  const autoSkipped =
    s.settings.automatic &&
    !pr.draft &&
    !trustedSource(pr, s.settings.trustedAuthors) &&
    s.prs.some((p) => p.id === pr.id) &&
    s.settings.repositories.some(
      (r) => sameRepo(r.name, pr.repo) && r.automatic,
    );
  return (
    <Layout
      header={
        <LayoutHeader padding={4} hasDivider>
          <VStack gap={3}>
            <HStack hAlign="between" vAlign="center">
              <Text color="secondary">
                {pr.repo} / #{pr.number}
              </Text>
              <IconButton
                label="Open pull request on GitHub"
                icon={<ArrowUpRight />}
                onClick={() => {
                  if (desktop) void action("open_github", { url: pr.url });
                  else window.open(pr.url, "_blank", "noopener,noreferrer");
                }}
              />
            </HStack>
            <Heading level={2}>{pr.title}</Heading>
            {tab === "overview" && (
              <HStack gap={2} wrap="wrap" vAlign="center">
                <Token
                  size="sm"
                  color="green"
                  label={
                    !s.prs.some((p) => p.id === pr.id)
                      ? "Saved review"
                      : pr.draft
                        ? "Draft"
                        : "Open"
                  }
                />
                <Text type="supporting" color="secondary">
                  {pr.author}
                </Text>
                <Code>{pr.headRef}</Code>
                <ArrowRight />
                <Code>{pr.baseRef}</Code>
                <Text className="diff-added">
                  +{pr.additions.toLocaleString()}
                </Text>
                <Text className="diff-removed">
                  −{pr.deletions.toLocaleString()}
                </Text>
              </HStack>
            )}
            <HStack gap={3} hAlign="between" vAlign="center" wrap="wrap">
              <VStack gap={1}>
                {review ? (
                  <HStack gap={2} vAlign="center">
                    <ReviewStatus status={reviewDisplayStatus(s, review)} />
                    <Timestamp
                      value={review.updatedAt}
                      format="relative_short"
                      isLive
                    />
                  </HStack>
                ) : (
                  <Text color="secondary">
                    Let Codex trace the changes and find what matters.
                  </Text>
                )}
              </VStack>
              {review && activeStatuses.includes(review.status) ? (
                <Button
                  label="Stop"
                  icon={<Square />}
                  size="sm"
                  onClick={() =>
                    void action("cancel_review", { reviewId: review.id })
                  }
                />
              ) : (
                (!review || canStartReview(s, pr.id)) && (
                  <Button
                    label={review ? "Review again" : "Start review"}
                    variant="primary"
                    size="sm"
                    icon={<Play />}
                    isDisabled={busy || !canStartReview(s, pr.id)}
                    isLoading={busy}
                    onClick={onStart}
                  />
                )
              )}
            </HStack>
            <TabList
              role="tablist"
              aria-label="Pull request details"
              value={tab}
              onChange={(value) => setTab(value as DetailTab)}
              layout="fill"
            >
              <Tab
                id="pr-tab-overview"
                value="overview"
                label="Overview"
                panelId="pr-overview"
              />
              <Tab
                value="chat"
                id="pr-tab-chat"
                label="Review chat"
                panelId="pr-chat"
                endContent={
                  review ? (
                    <Badge
                      label={
                        review.messages.filter((m) => m.role !== "system")
                          .length
                      }
                    />
                  ) : undefined
                }
              />
              <Tab
                value="files"
                id="pr-tab-files"
                label="Files changed"
                panelId="pr-files"
                endContent={<Badge label={pr.changedFiles} />}
              />
            </TabList>
          </VStack>
        </LayoutHeader>
      }
      content={(["overview", "chat", "files"] as const).map((panel) => (
        <LayoutContent
          key={panel}
          padding={0}
          isScrollable={tab !== "chat"}
          role="tabpanel"
          id={`pr-${panel}`}
          aria-labelledby={`pr-tab-${panel}`}
          hidden={panel !== tab}
          style={panel !== tab ? { display: "none" } : undefined}
        >
          {panel === tab && (
            <>
              {(tab !== "chat" || !review) && reviewBlocked && (
                <Banner
                  status="warning"
                  title={reviewBlocked}
                  endContent={
                    <Button label="Open Settings" onClick={onSettings} />
                  }
                />
              )}
              {autoSkipped && (
                <Banner
                  status="warning"
                  title={
                    pr.author && sameRepository(pr)
                      ? `Autopilot skips pull requests from ${pr.author} because it isn’t an owner, member, or collaborator of this repository. Trust it if it opens pull requests for you, like a coding agent’s bot.`
                      : "Autopilot skips this pull request because it comes from a fork. You can still review it manually."
                  }
                  endContent={
                    pr.author && sameRepository(pr) ? (
                      <Button
                        label={`Trust ${pr.author}`}
                        onClick={() =>
                          void updateSettings({
                            trustedAuthors: [
                              ...s.settings.trustedAuthors,
                              pr.author,
                            ],
                          })
                        }
                      />
                    ) : undefined
                  }
                />
              )}
              {tab === "overview" ? (
                <VStack gap={6} padding={5}>
                  <LoopPanel prId={pr.id} />
                  <VStack gap={3}>
                    <Heading level={3}>Description</Heading>
                    <SafeMarkdown>
                      {pr.body || "No description was provided."}
                    </SafeMarkdown>
                  </VStack>
                  {review?.report && (
                    <Card>
                      <VStack gap={3}>
                        <HStack hAlign="between" gap={3}>
                          <Heading level={3}>Review report</Heading>
                          <ReviewStatus
                            status={reviewDisplayStatus(s, review)}
                          />
                        </HStack>
                        <Text color="secondary">
                          {reviewDisplayStatus(s, review) === "outdated"
                            ? "The pull request changed after this report. Read the saved findings, then review the current commits before publishing."
                            : "The report is saved with this run. Open its chat to read the findings and available actions."}
                        </Text>
                        <Text type="supporting" color="secondary">
                          {reportSelectionLabel(review)}
                        </Text>
                        <HStack>
                          <Button
                            label="Read the review"
                            variant="ghost"
                            onClick={() => setTab("chat")}
                          />
                        </HStack>
                      </VStack>
                    </Card>
                  )}
                  <VStack gap={3}>
                    <HStack hAlign="between" vAlign="center">
                      <Heading level={3}>New review environment</Heading>
                      <Button
                        label="Configure"
                        variant="ghost"
                        size="sm"
                        onClick={onRepositories}
                      />
                    </HStack>
                    <Grid columns={{ minWidth: 170 }} gap={4}>
                      {[
                        ["Workspace", "Isolated worktree"],
                        ["Execution", "Codex sandbox"],
                        [
                          "Model",
                          s.models.find((m) => m.model === s.settings.model)
                            ?.displayName ||
                            s.settings.model ||
                            "Choose a model",
                        ],
                        ["Reasoning", s.settings.effort || "Choose an effort"],
                      ].map(([label, value]) => (
                        <VStack gap={1} key={label}>
                          <Text type="supporting" color="secondary">
                            {label}
                          </Text>
                          <Text weight="medium">{value}</Text>
                        </VStack>
                      ))}
                    </Grid>
                  </VStack>
                  {!!pr.labels.length && (
                    <HStack gap={2} wrap="wrap">
                      {pr.labels.map((label) => (
                        <Token key={label} label={label} />
                      ))}
                    </HStack>
                  )}
                </VStack>
              ) : tab === "chat" ? (
                <Suspense fallback={<Spinner label="Loading chat…" />}>
                  <Chat
                    key={review?.id ?? pr.id}
                    pr={pr}
                    review={review}
                    onStart={onStart}
                    onPublish={onPublish}
                    onHistory={onHistory}
                  />
                </Suspense>
              ) : (
                <Files key={`${pr.id}:${pr.headSha}:${pr.baseSha}`} pr={pr} />
              )}
            </>
          )}
        </LayoutContent>
      ))}
      footer={
        <LayoutFooter padding={3} hasDivider>
          <HStack hAlign="between" gap={2} vAlign="center">
            <Text type="supporting" color="secondary">
              {live
                ? "Local workspace · isolated reviews"
                : "Demo workspace · no external actions"}
            </Text>
            {review?.status === "posted" ? (
              <ReviewActions review={review} exportable={false} />
            ) : (
              <Button
                label="Publish review"
                size="sm"
                isDisabled={
                  !review || !canPublish(review, publishTarget(s, pr))
                }
                onClick={() => review && onPublish(review)}
              />
            )}
          </HStack>
        </LayoutFooter>
      }
    />
  );
}
