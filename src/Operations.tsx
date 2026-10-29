import {
  Button,
  Card,
  EmptyState,
  Heading,
  HStack,
  List,
  ListItem,
  StatusDot,
  Text,
  Timestamp,
  VStack,
} from "@astryxdesign/core";
import { FolderGit2, Workflow } from "lucide-react";
import { useOrbit } from "./store";
import {
  checkNames,
  sameRepo,
  shortSha,
  type AttentionHandlers,
  type CiState,
  type Problem,
  type RepoCi,
  type Snapshot,
} from "./types";

/** How many closed problems the page lists. */
const RECENT = 10;
/** How many of a problem's latest events it shows. */
const EVENTS = 5;

const dots: Record<
  CiState,
  { variant: "success" | "error" | "accent" | "neutral"; label: string }
> = {
  passing: { variant: "success", label: "Passing" },
  failing: { variant: "error", label: "Failing" },
  running: { variant: "accent", label: "Running" },
  none: { variant: "neutral", label: "No checks" },
};

/** Where a repository's default branch's checks stand, in a sentence. */
export function branchSummary(ci: RepoCi) {
  const branch = ci.branch || "the default branch";
  if (!ci.head.sha) return "No commits to check yet.";
  const at = shortSha(ci.head.sha);
  switch (ci.head.state) {
    case "failing":
      return `Failing on ${branch} at ${at}: ${checkNames(ci.head.failing)}.`;
    case "running":
      return `Checks are running on ${branch} at ${at}.`;
    case "passing":
      return `Passing on ${branch} at ${at}.`;
    default:
      return `No checks ran on ${branch} at ${at}.`;
  }
}

/** A problem's name: its repository and branch, or its pull request. */
function title(s: Snapshot, p: Problem) {
  if (p.kind === "default_branch")
    return `${p.repo}: ${p.branch || "default branch"}`;
  const pr = s.prs.find((pr) => pr.id === p.prId);
  return pr ? `${p.prId} · ${pr.title}` : (p.prId ?? p.repo);
}

function Repositories({ go }: { go: AttentionHandlers }) {
  const { snapshot: s } = useOrbit();
  const repos = s.settings.repositories;
  if (!repos.length)
    return (
      <EmptyState
        icon={<FolderGit2 />}
        title="No repositories connected"
        description="Connect a repository and Orbit watches its CI after each sync."
        actions={
          <Button
            label="Open repositories"
            variant="primary"
            onClick={go.onRepositories}
          />
        }
      />
    );
  return (
    <Card padding={0}>
      <List hasDividers aria-label="CI by repository">
        {repos.map((r) => {
          const ci = s.ci?.find((c) => sameRepo(c.repo, r.name));
          const failingPulls = (s.problems ?? []).filter(
            (p) =>
              !p.closedAt &&
              p.kind === "pull_request" &&
              sameRepo(p.repo, r.name),
          ).length;
          const dot = ci?.head.sha ? dots[ci.head.state] : dots.none;
          return (
            <ListItem
              key={r.name}
              startContent={
                <StatusDot variant={dot.variant} label={dot.label} />
              }
              label={<Text weight="medium">{r.name}</Text>}
              description={
                <VStack gap={1}>
                  <Text color="secondary">
                    {ci && (ci.head.sha || !ci.error)
                      ? branchSummary(ci)
                      : "Not read yet."}
                  </Text>
                  {failingPulls > 0 && (
                    <Text color="secondary">
                      {failingPulls === 1
                        ? "1 pull request is failing."
                        : `${failingPulls} pull requests are failing.`}
                    </Text>
                  )}
                  {ci?.error && (
                    <HStack gap={2} vAlign="center">
                      <StatusDot variant="warning" label="Not read" />
                      <Text color="secondary">
                        Couldn’t read CI: {ci.error}
                      </Text>
                    </HStack>
                  )}
                </VStack>
              }
              endContent={
                ci?.checkedAt ? (
                  <Timestamp value={ci.checkedAt} format="relative_short" />
                ) : undefined
              }
            />
          );
        })}
      </List>
    </Card>
  );
}

function Timeline({ p }: { p: Problem }) {
  return (
    <VStack gap={1}>
      {p.timeline.slice(-EVENTS).map((e, i) => (
        <HStack key={`${e.at}-${i}`} gap={2} vAlign="start">
          <Timestamp value={e.at} format="relative_short" type="supporting" />
          <Text type="supporting" color="secondary">
            {e.text}
          </Text>
        </HStack>
      ))}
    </VStack>
  );
}

function OpenProblems({ go }: { go: AttentionHandlers }) {
  const { snapshot: s } = useOrbit();
  const open = (s.problems ?? [])
    .filter((p) => !p.closedAt)
    .sort(
      (a, b) =>
        Number(a.kind !== "default_branch") -
          Number(b.kind !== "default_branch") ||
        b.openedAt.localeCompare(a.openedAt),
    );
  if (!open.length)
    return (
      <Card variant="green" padding={3}>
        <HStack gap={3} vAlign="center">
          <StatusDot variant="success" label="All clear" />
          <Text weight="medium">All clear. Nothing is failing.</Text>
        </HStack>
      </Card>
    );
  return (
    <Card padding={0}>
      <List hasDividers aria-label="Open problems">
        {open.map((p) => {
          const pr = s.prs.find((pr) => pr.id === p.prId);
          const yours = p.kind === "default_branch";
          return (
            <ListItem
              key={p.id}
              startContent={
                <StatusDot
                  variant={yours ? "error" : "warning"}
                  label={yours ? "Needs you" : "Author’s to fix"}
                />
              }
              label={
                <Text weight="medium" maxLines={2}>
                  {title(s, p)}
                </Text>
              }
              description={
                <VStack gap={2}>
                  <Text>
                    {yours
                      ? `Failing: ${checkNames(p.failing)}.`
                      : `Failing: ${checkNames(p.failing)}. Its author’s to fix; Orbit only lists it.`}
                  </Text>
                  <Timeline p={p} />
                </VStack>
              }
              onClick={pr ? () => go.onChoose(pr) : undefined}
              endContent={
                <Timestamp value={p.openedAt} format="relative_short" />
              }
            />
          );
        })}
      </List>
    </Card>
  );
}

function RecentlyFixed() {
  const { snapshot: s } = useOrbit();
  const closed = (s.problems ?? [])
    .filter((p) => p.closedAt)
    .sort((a, b) => (b.closedAt ?? "").localeCompare(a.closedAt ?? ""))
    .slice(0, RECENT);
  if (!closed.length) return null;
  return (
    <VStack gap={3}>
      <Heading level={3}>Recently closed</Heading>
      <Card padding={0}>
        <List hasDividers aria-label="Recently closed">
          {closed.map((p) => (
            <ListItem
              key={p.id}
              startContent={<StatusDot variant="neutral" label="Closed" />}
              label={
                <Text weight="medium" maxLines={2}>
                  {title(s, p)}
                </Text>
              }
              description={<Text color="secondary">{p.closedReason}</Text>}
              endContent={
                p.closedAt ? (
                  <Timestamp value={p.closedAt} format="relative_short" />
                ) : undefined
              }
            />
          ))}
        </List>
      </Card>
    </VStack>
  );
}

/** CI on every connected repository, and what is failing. */
export function Operations(go: AttentionHandlers) {
  const { snapshot: s, updateSettings } = useOrbit();
  const watching = s.settings.watchCi ?? true;
  return (
    <VStack gap={6} padding={6} maxWidth={1100} className="content-page">
      <VStack gap={2}>
        <Heading level={2}>Operations</Heading>
        <Text color="secondary">
          Orbit reads CI after each sync. A failing default branch needs you; a
          failing pull request is its author’s to fix, so it is only listed.
        </Text>
      </VStack>
      {!watching ? (
        <EmptyState
          icon={<Workflow />}
          title="Watching CI is off"
          description="Turn it on to see each repository’s checks here and hear when a default branch fails."
          actions={
            <Button
              label="Turn on"
              variant="primary"
              onClick={() => void updateSettings({ watchCi: true })}
            />
          }
        />
      ) : (
        <>
          <VStack gap={3}>
            <Heading level={3}>Open problems</Heading>
            <OpenProblems go={go} />
          </VStack>
          <VStack gap={3}>
            <Heading level={3}>CI by repository</Heading>
            <Repositories go={go} />
          </VStack>
          <RecentlyFixed />
        </>
      )}
    </VStack>
  );
}
