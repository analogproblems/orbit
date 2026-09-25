import type { RefObject } from "react";
import {
  Badge,
  Button,
  EmptyState,
  Heading,
  HStack,
  IconButton,
  Kbd,
  Layout,
  LayoutContent,
  LayoutFooter,
  LayoutHeader,
  List,
  ListItem,
  SegmentedControl,
  SegmentedControlItem,
  Selector,
  StatusDot,
  StackItem,
  Text,
  TextInput,
  Timestamp,
  VStack,
} from "@astryxdesign/core";
import { Inbox as InboxIcon, Plus, RefreshCw, Search } from "lucide-react";
import { useOrbit } from "./store";
import { latestReview, reviewDisplayStatus, type PullRequest } from "./types";
import { ReviewStatus } from "./ReviewStatus";

export function Inbox({
  selected,
  search,
  setSearch,
  filter,
  setFilter,
  repoFilter,
  setRepoFilter,
  searchRef,
  onChoose,
  onRepositories,
}: {
  selected?: string;
  search: string;
  setSearch: (v: string) => void;
  filter: string;
  setFilter: (v: string) => void;
  repoFilter: string;
  setRepoFilter: (v: string) => void;
  searchRef: RefObject<HTMLInputElement | null>;
  onChoose: (p: PullRequest) => void;
  onRepositories: () => void;
}) {
  const { snapshot: s, action } = useOrbit();
  const visible = s.prs.filter(
    (p) =>
      (filter !== "requested" || p.reviewRequested) &&
      (filter !== "authored" || p.author === s.githubUser) &&
      (repoFilter === "all" || p.repo === repoFilter) &&
      `${p.title} ${p.repo} ${p.number} ${p.author}`
        .toLowerCase()
        .includes(search.toLowerCase()),
  );
  return (
    <Layout
      header={
        <LayoutHeader padding={4}>
          <VStack gap={3}>
            <HStack hAlign="between" vAlign="center">
              <Heading level={2}>Your review inbox</Heading>
              <IconButton
                label="Refresh pull requests"
                icon={<RefreshCw />}
                isLoading={s.polling}
                isDisabled={s.polling}
                onClick={() => void action("poll_now")}
              />
            </HStack>
            <HStack gap={2} vAlign="center">
              <StackItem size="fill">
                <TextInput
                  width="100%"
                  ref={searchRef}
                  label="Search pull requests"
                  isLabelHidden
                  placeholder="Search pull requests…"
                  value={search}
                  onChange={setSearch}
                  startIcon={<Search />}
                />
              </StackItem>
              <Kbd keys="mod+k" />
            </HStack>
            <SegmentedControl
              label="Filter pull requests"
              value={filter}
              onChange={setFilter}
              layout="fill"
            >
              {[
                ["all", "All"],
                ["requested", "For you"],
                ["authored", "Authored"],
              ].map(([value, label]) => (
                <SegmentedControlItem key={value} value={value} label={label} />
              ))}
            </SegmentedControl>
            <Selector
              label="Filter by repository"
              isLabelHidden
              value={repoFilter}
              onChange={setRepoFilter}
              options={[
                { value: "all", label: "All repos" },
                ...[...new Set(s.prs.map((p) => p.repo))].map((repo) => ({
                  value: repo,
                  label: repo,
                })),
              ]}
            />
            <HStack hAlign="between" vAlign="center">
              <Text type="supporting" color="secondary">
                Open pull requests
              </Text>
              <Badge label={visible.length} />
            </HStack>
          </VStack>
        </LayoutHeader>
      }
      content={
        <LayoutContent padding={2}>
          <List density="spacious" hasDividers>
            {visible.map((p) => {
              const r = latestReview(s.reviews, p.id);
              return (
                <ListItem
                  key={p.id}
                  label={
                    <Text weight="medium" maxLines={3}>
                      {p.title}
                    </Text>
                  }
                  isSelected={selected === p.id}
                  onClick={() => onChoose(p)}
                  description={
                    <VStack gap={2} paddingBlockStart={2}>
                      <HStack gap={2} wrap="wrap">
                        <Text type="supporting" color="secondary">
                          {p.repo.split("/")[1]} #{p.number} · {p.author}
                        </Text>
                        <Timestamp
                          value={p.updatedAt}
                          format="relative_short"
                          isLive
                        />
                      </HStack>
                      <HStack>
                        <ReviewStatus
                          status={
                            r
                              ? reviewDisplayStatus(s, r)
                              : p.draft
                                ? "draft"
                                : "unreviewed"
                          }
                        />
                      </HStack>
                    </VStack>
                  }
                />
              );
            })}
          </List>
          {!visible.length && (
            <EmptyState
              isCompact
              icon={<InboxIcon />}
              title={
                search
                  ? "No matching pull requests"
                  : s.settings.repositories.length
                    ? "Your inbox is clear"
                    : "No repositories connected"
              }
              description={
                s.githubError
                  ? "GitHub could not sync. Check the connection in Settings."
                  : search
                    ? "Try a different title, repository, or author."
                    : s.settings.repositories.length
                      ? "Open pull requests in your connected repositories will appear here."
                      : "Connect a repository to see its open pull requests here."
              }
              actions={
                !s.settings.repositories.length && !search ? (
                  <Button
                    label="Connect a repository"
                    icon={<Plus />}
                    onClick={onRepositories}
                  />
                ) : undefined
              }
            />
          )}
        </LayoutContent>
      }
      footer={
        <LayoutFooter padding={3} hasDivider>
          <VStack gap={1}>
            <HStack gap={2} vAlign="center">
              <StatusDot
                variant={s.githubError ? "warning" : "success"}
                label={
                  s.polling
                    ? "Syncing with GitHub…"
                    : s.githubError
                      ? "Sync needs attention"
                      : "Synced"
                }
              />
              <Text type="supporting" color="secondary">
                {s.polling
                  ? "Syncing with GitHub…"
                  : s.githubError
                    ? "Sync needs attention"
                    : "Synced"}
              </Text>
            </HStack>
            <HStack gap={2}>
              <Text type="supporting" color="secondary">
                {s.lastPoll ? (
                  <Timestamp
                    value={s.lastPoll}
                    format="relative_short"
                    isLive
                  />
                ) : (
                  "Never"
                )}{" "}
                · Every{" "}
                {s.settings.pollSeconds < 60
                  ? `${s.settings.pollSeconds}s`
                  : `${Math.round(s.settings.pollSeconds / 60)}m`}
              </Text>
            </HStack>
          </VStack>
        </LayoutFooter>
      }
    />
  );
}
