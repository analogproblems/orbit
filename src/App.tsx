import { lazy, Suspense, useEffect, useRef, useState } from "react";
import {
  AppShell,
  Badge,
  Banner,
  Button,
  Code,
  CodeBlock,
  Dialog,
  DialogHeader,
  EmptyState,
  Heading,
  HStack,
  IconButton,
  Kbd,
  Layout,
  LayoutContent,
  LayoutFooter,
  LayoutHeader,
  LayoutPanel,
  SideNav,
  SideNavHeading,
  SideNavItem,
  SideNavSection,
  Spinner,
  StatusDot,
  Switch,
  Text,
  TopNav,
  VStack,
} from "@astryxdesign/core";
import { useMediaQuery } from "@astryxdesign/core/hooks";
import {
  Activity,
  BellRing,
  Bug,
  FolderGit2,
  GitPullRequest,
  Inbox as InboxIcon,
  Repeat,
  Scale,
  Send,
  Settings2,
  ShieldCheck,
  SlidersHorizontal,
  Workflow,
} from "lucide-react";
import { useOrbit, desktop, live, remote } from "./store";
import { startBrowserNotices } from "./notices";
import { listen } from "@tauri-apps/api/event";
import {
  activeStatuses,
  cliStatus,
  providerLabels,
  attentionAction,
  autopilotSummary,
  buddyOf,
  canPublish,
  latestReview,
  publishTarget,
  scanActive,
  type Attention,
  type PullRequest,
  type Review,
  type SettingsTab,
} from "./types";
import { Inbox } from "./Inbox";
import { Detail, type DetailTab } from "./Detail";
import { SaveFeedback } from "./SaveFeedback";
import { PublishPreview } from "./PublishPreview";
import { UsageMeter } from "./Usage";
import { BuddyCard, BuddyPage } from "./Buddy";

const Live = lazy(() => import("./Live").then((m) => ({ default: m.Live })));
const FindingsPage = lazy(() =>
  import("./FindingsPage").then((m) => ({ default: m.FindingsPage })),
);
const Loops = lazy(() => import("./Loops").then((m) => ({ default: m.Loops })));
const DecisionsPage = lazy(() =>
  import("./Decisions").then((m) => ({ default: m.DecisionsPage })),
);
const Operations = lazy(() =>
  import("./Operations").then((m) => ({ default: m.Operations })),
);
const ScansPage = lazy(() =>
  import("./Scans").then((m) => ({ default: m.ScansPage })),
);
const SettingsPage = lazy(() =>
  import("./Settings").then((m) => ({ default: m.SettingsPage })),
);
const Repositories = lazy(() =>
  import("./Repositories").then((m) => ({ default: m.Repositories })),
);

type Page =
  | "inbox"
  | "live"
  | "operations"
  | "loops"
  | "findings"
  | "decisions"
  | "scans"
  | "repositories"
  | "settings"
  | "buddy";
export default function App() {
  const {
    snapshot: s,
    loaded,
    error,
    clearError,
    init,
    action,
    updateSettings: save,
    saves,
  } = useOrbit();
  const [page, setPage] = useState<Page>("inbox");
  // The loop whose rounds are open on the loops page.
  const [loop, setLoop] = useState<string | null>(null);
  // The finding whose page is open on the findings page.
  const [finding, setFinding] = useState<string | null>(null);
  // The pull request whose chapter is open on the decisions page.
  const [chapter, setChapter] = useState<string | null>(null);
  // Settings, open at one of its pages.
  const openSettings = (tab: SettingsTab) => {
    useOrbit.getState().setSettingsTab(tab);
    setPage("settings");
  };
  const openLoops = () => {
    setLoop(null);
    setPage("loops");
  };
  const [selected, setSelected] = useState<string | null>(null);
  const [selectedReview, setSelectedReview] = useState<string | null>(null);
  const [tab, setTab] = useState<DetailTab>("overview");
  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState("all");
  const [repoFilter, setRepoFilter] = useState("all");
  const [collapsed, setCollapsed] = useState(() => window.innerWidth < 1100);
  // Below the mobile menu breakpoint (Astryx md), the inbox shows its list
  // and the opened pull request one at a time instead of side by side.
  const phone = useMediaQuery("(width < 768px)");
  const [showingPr, setShowingPr] = useState(false);
  const [publish, setPublish] = useState<Review | null>(null);
  const [confirmSensitive, setConfirmSensitive] = useState(false);
  const [publishError, setPublishError] = useState<string | null>(null);
  const [focusSearch, setFocusSearch] = useState(0);
  const [busy, setBusy] = useState(false);
  const [approvalOpen, setApprovalOpen] = useState(false);
  // The request the dialog was opened for, so it never shows another one.
  const [openedApproval, setOpenedApproval] = useState<{
    reviewId: string;
    id: string | number;
  } | null>(null);
  const [answeringApproval, setAnsweringApproval] = useState(false);
  const [approvalError, setApprovalError] = useState<{
    id: string | number;
    error: string;
  } | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  // Opens the item of a notification when Orbit is brought forward for it.
  const openNotified = useRef<(a: Attention) => void>(() => {});
  useEffect(() => {
    if (!desktop) return;
    const off = listen<Attention>("orbit-open", (e) =>
      openNotified.current(e.payload),
    );
    return () => void off.then((f) => f());
  }, []);
  // In a browser, a tap on one of Orbit's notifications opens its item too.
  useEffect(() => {
    if (!remote) return;
    return startBrowserNotices((item) => openNotified.current(item));
  }, []);
  useEffect(() => {
    let off: (() => void) | undefined;
    let gone = false;
    void init().then((f) => {
      if (gone) return f();
      off = f;
      // Live is the first page under autopilot; the inbox otherwise.
      if (useOrbit.getState().snapshot.settings.automatic)
        setPage((p) => (p === "inbox" ? "live" : p));
    });
    return () => {
      gone = true;
      off?.();
    };
  }, [init]);
  useEffect(() => {
    const narrow = window.matchMedia("(max-width: 1099px)");
    const change = () => setCollapsed(narrow.matches);
    narrow.addEventListener("change", change);
    return () => narrow.removeEventListener("change", change);
  }, []);
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (
        document.querySelector(
          'dialog[open], [role="dialog"]:not(dialog), [role="alertdialog"]:not(dialog)',
        )
      )
        return;
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPage("inbox");
        setShowingPr(false);
        setFocusSearch((v) => v + 1);
      } else if ((e.metaKey || e.ctrlKey) && e.key === ",") {
        e.preventDefault();
        setPage("settings");
      }
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, []);
  useEffect(() => {
    if (page === "inbox" && focusSearch) searchRef.current?.focus();
  }, [page, focusSearch, loaded]);
  const knownPrs = [...s.prs, ...(s.historyPrs ?? [])];
  const chosen = knownPrs.find((p) => p.id === selected);
  const pr = chosen ?? s.prs[0];
  // Read after an await, to see which pull request is showing by then.
  const shownPr = useRef<string | undefined>(undefined);
  const shownId = pr?.id;
  useEffect(() => {
    shownPr.current = shownId;
  }, [shownId]);
  const review =
    (selectedReview
      ? s.reviews.find((r) => r.id === selectedReview && r.prId === pr?.id)
      : undefined) ?? latestReview(s.reviews, pr?.id ?? "");
  const active = s.reviews.filter((r) => activeStatuses.includes(r.status));
  const running = active.filter((r) => r.status !== "queued").length;
  const publishRun = s.reviews.find((r) => r.id === publish?.id);
  const publishPr = knownPrs.find((p) => p.id === publishRun?.prId);
  const publishEligible =
    !!publishRun &&
    !!publishPr &&
    canPublish(publishRun, publishTarget(s, publishPr));
  // The banner offers the request of the review on screen, else the oldest.
  const offeredApproval =
    s.approvals.find((a) => page === "inbox" && a.reviewId === review?.id) ??
    s.approvals[0];
  const approval = s.approvals.find(
    (a) =>
      a.reviewId === openedApproval?.reviewId && a.id === openedApproval?.id,
  );
  const openApproval = (reviewId: string, id: string | number) => {
    setOpenedApproval({ reviewId, id });
    setApprovalError(null);
    setApprovalOpen(true);
  };
  const needsYou = s.attention?.length ?? 0;
  const ciFailing = (s.attention ?? []).filter(
    (a) => a.kind === "ci_failing",
  ).length;
  const runsWaiting = (s.attention ?? []).filter(
    (a) => a.kind === "runs_waiting",
  ).length;
  const choose = (p: PullRequest, r?: Review) => {
    setSelected(p.id);
    setSelectedReview(r?.id ?? null);
    setTab(r ? "chat" : "overview");
    setPage("inbox");
    setShowingPr(true);
  };
  // Bit's setup help, while Bit is on.
  const setUpWithBit = buddyOf(s.settings).enabled
    ? () => {
        setPage("buddy");
        void action("ask_buddy", { question: "setup" });
      }
    : undefined;
  const handlers = {
    onChoose: choose,
    onRepositories: () => setPage("repositories"),
    onSettings: () => openSettings("connections"),
    onApproval: openApproval,
    onPublish: setPublish,
    onOperations: () => setPage("operations"),
    onBuddy: () => setPage("buddy"),
  };
  useEffect(() => {
    openNotified.current = (item) => {
      // Only an item that is still open.
      if (s.attention?.some((a) => a.key === item.key))
        attentionAction(s, item, handlers)?.();
    };
  });
  const closePublish = () => {
    setPublish(null);
    setConfirmSensitive(false);
    setPublishError(null);
  };
  const changePublishOpen = (open: boolean) => {
    if (!open && !busy) closePublish();
  };
  const start = async () => {
    if (!pr || busy) return;
    setBusy(true);
    const started = pr.id;
    const result = await action<string>("start_review", { prId: started });
    // Only open the chat if the same pull request is still showing.
    if (result.ok && shownPr.current === started) {
      setSelectedReview(result.data ?? null);
      setTab("chat");
    }
    setBusy(false);
  };
  const titles: Record<Page, string> = {
    inbox: "All pull requests",
    live: "Live",
    operations: "Operations",
    loops: "Review loops",
    findings: "Findings",
    decisions: "Decisions",
    scans: "Security scans",
    repositories: "Repositories",
    settings: "Settings",
    buddy: buddyOf(s.settings).name,
  };
  const inbox = (
    <Inbox
      selected={phone ? chosen?.id : pr?.id}
      search={search}
      setSearch={setSearch}
      filter={filter}
      setFilter={setFilter}
      repoFilter={repoFilter}
      setRepoFilter={setRepoFilter}
      searchRef={searchRef}
      onChoose={choose}
      onRepositories={() => setPage("repositories")}
      onSetUp={setUpWithBit}
    />
  );
  const detail = (p: PullRequest, onBack?: () => void) => (
    <Detail
      pr={p}
      review={review}
      tab={tab}
      setTab={setTab}
      busy={busy}
      onStart={() => void start()}
      onPublish={setPublish}
      onHistory={setSelectedReview}
      onSettings={() => openSettings("reviewers")}
      onRepositories={() => setPage("repositories")}
      onBack={onBack}
    />
  );
  return (
    <>
      <AppShell
        contentPadding={0}
        mobileNav={{ breakpoint: "md" }}
        variant="surface"
        sideNav={
          <SideNav
            aria-label="Main navigation"
            collapsible={{
              isCollapsed: collapsed,
              onCollapsedChange: setCollapsed,
            }}
            header={
              <SideNavHeading
                heading="orbit."
                icon={
                  <img className="orbit-logo" src="/orbit.svg" alt="Orbit" />
                }
                subheading={live ? "Local workspace" : "Demo workspace"}
              />
            }
            footer={
              <VStack gap={3} padding={3}>
                <BuddyCard
                  collapsed={collapsed}
                  isSelected={page === "buddy"}
                  onOpen={() => setPage("buddy")}
                />
                {!collapsed && (
                  <>
                    <UsageMeter usage={s.usage} />
                    <HStack gap={2} vAlign="center">
                      <StatusDot
                        variant={s.codexReady ? "success" : "warning"}
                        label="Codex connection"
                      />
                      <Text type="supporting" color="secondary">
                        Codex agent ·{" "}
                        {s.codexReady ? "Connected" : "Not connected"}
                      </Text>
                    </HStack>
                    {(["claude", "antigravity"] as const)
                      .filter((provider) =>
                        s.settings.repositories.some(
                          (r) => r.provider === provider,
                        ),
                      )
                      .map((provider) => (
                        <HStack gap={2} vAlign="center" key={provider}>
                          <StatusDot
                            variant={
                              cliStatus(s, provider)?.ready
                                ? "success"
                                : "warning"
                            }
                            label={`${providerLabels[provider]} connection`}
                          />
                          <Text type="supporting" color="secondary">
                            {providerLabels[provider]} ·{" "}
                            {cliStatus(s, provider)?.ready
                              ? "Connected"
                              : "Not connected"}
                          </Text>
                        </HStack>
                      ))}
                    <HStack gap={2} vAlign="center">
                      <StatusDot
                        variant={
                          s.githubUser && !s.githubError ? "success" : "warning"
                        }
                        label="GitHub connection"
                      />
                      <Text type="supporting" color="secondary">
                        GitHub ·{" "}
                        {s.githubUser && !s.githubError
                          ? "Connected"
                          : "Not connected"}
                      </Text>
                    </HStack>
                    <Text type="supporting" color="secondary">
                      {live
                        ? s.githubUser || "Local workspace"
                        : "Demo workspace"}
                    </Text>
                  </>
                )}
                <SideNavItem
                  label="Settings"
                  icon={<Settings2 />}
                  isSelected={page === "settings"}
                  endContent={<Kbd keys="mod+," />}
                  onClick={() => setPage("settings")}
                />
              </VStack>
            }
          >
            <SideNavSection title="Your workspace">
              {needsYou > 0 && (
                <SideNavItem
                  label="Needs you"
                  icon={<BellRing />}
                  endContent={<Badge variant="warning" label={needsYou} />}
                  onClick={openLoops}
                />
              )}
              <SideNavItem
                label="Live"
                icon={<Activity />}
                isSelected={page === "live"}
                endContent={
                  running > 0 ? (
                    <StatusDot
                      variant="accent"
                      label={`${running} running`}
                      isPulsing
                    />
                  ) : undefined
                }
                onClick={() => setPage("live")}
              />
              <SideNavItem
                label="Operations"
                icon={<Workflow />}
                isSelected={page === "operations"}
                endContent={
                  ciFailing > 0 ? (
                    <StatusDot variant="error" label="CI failing" />
                  ) : runsWaiting > 0 ? (
                    <StatusDot variant="warning" label="Runs waiting" />
                  ) : undefined
                }
                onClick={() => setPage("operations")}
              />
              <SideNavItem
                label="Review loops"
                icon={<Repeat />}
                isSelected={page === "loops"}
                endContent={
                  active.length ? <Badge label={active.length} /> : undefined
                }
                onClick={openLoops}
              />
              <SideNavItem
                label="Findings"
                icon={<Bug />}
                isSelected={page === "findings"}
                onClick={() => {
                  setFinding(null);
                  setPage("findings");
                }}
              />
              <SideNavItem
                label="Decisions"
                icon={<Scale />}
                isSelected={page === "decisions"}
                onClick={() => {
                  setChapter(null);
                  setPage("decisions");
                }}
              />
              <SideNavItem
                label="Security scans"
                icon={<ShieldCheck />}
                isSelected={page === "scans"}
                endContent={
                  s.scans?.some(scanActive) ? (
                    <StatusDot variant="accent" label="Scanning" isPulsing />
                  ) : undefined
                }
                onClick={() => setPage("scans")}
              />
              <SideNavItem
                label="All pull requests"
                icon={<GitPullRequest />}
                isSelected={page === "inbox"}
                endContent={<Badge label={s.prs.length} />}
                onClick={() => {
                  setPage("inbox");
                  setShowingPr(false);
                }}
              />
              <SideNavItem
                label="Repositories"
                icon={<FolderGit2 />}
                isSelected={page === "repositories"}
                onClick={() => setPage("repositories")}
              />
            </SideNavSection>
            <SideNavSection
              title="Recent reviews"
              endContent={<Badge label={s.reviews.length} />}
            >
              {s.reviews.slice(0, 5).map((r) => {
                const p = knownPrs.find((p) => p.id === r.prId);
                return (
                  <SideNavItem
                    key={r.id}
                    label={p?.title ?? r.prId}
                    icon={<GitPullRequest />}
                    onClick={() => (p ? choose(p, r) : openLoops())}
                  />
                );
              })}
              {!s.reviews.length && !collapsed && (
                <Text color="secondary" type="supporting">
                  Your review chats will appear here.
                </Text>
              )}
            </SideNavSection>
          </SideNav>
        }
        topNav={
          <TopNav
            label="Workspace controls"
            startContent={<Heading level={1}>{titles[page]}</Heading>}
            endContent={
              <HStack gap={3} vAlign="center">
                <Switch
                  label="Automatic PR reviews"
                  value={s.settings.automatic}
                  isDisabled={saves.automatic?.state === "saving"}
                  onChange={(automatic) => void save({ automatic })}
                  description={
                    s.settings.automatic ? autopilotSummary(s) : "Manual mode"
                  }
                />
                <IconButton
                  label="Open settings"
                  icon={<SlidersHorizontal />}
                  onClick={() => openSettings("automation")}
                />
              </HStack>
            }
          />
        }
      >
        <Layout
          header={
            <LayoutHeader padding={0}>
              <VStack>
                {!live && (
                  <Banner
                    status="info"
                    container="section"
                    title="Interactive demo"
                    description="Sample pull requests. Reviews and comments are simulated."
                    endContent={
                      <Button
                        label="Connect your workspace"
                        size="sm"
                        onClick={() => openSettings("connections")}
                      />
                    }
                  />
                )}
                {page !== "settings" &&
                  Object.entries(saves).some(
                    ([key, value]) =>
                      !key.startsWith("repository:") && value.state === "error",
                  ) && <SaveFeedback />}
                {error && (
                  <Banner
                    key={error}
                    status="error"
                    title={error}
                    isDismissable
                    dismissLabel="Dismiss error"
                    onDismiss={clearError}
                    container="section"
                  />
                )}
                {offeredApproval && (
                  <Banner
                    status="warning"
                    title={
                      s.approvals.length > 1
                        ? `${s.approvals.length} requests need your approval.`
                        : "A review needs your approval to continue."
                    }
                    container="section"
                    endContent={
                      <Button
                        label="Review request"
                        onClick={() =>
                          openApproval(
                            offeredApproval.reviewId,
                            offeredApproval.id,
                          )
                        }
                      />
                    }
                  />
                )}
              </VStack>
            </LayoutHeader>
          }
          content={
            !loaded ? (
              <VStack padding={6}>
                <Spinner label="Loading your workspace…" />
              </VStack>
            ) : page === "inbox" ? (
              phone ? (
                showingPr && chosen ? (
                  detail(chosen, () => setShowingPr(false))
                ) : (
                  inbox
                )
              ) : (
                <Layout
                  start={
                    <LayoutPanel
                      className="inbox-panel"
                      label="Pull request inbox"
                      role="region"
                      width="clamp(280px, 27vw, 380px)"
                      padding={0}
                      hasDivider
                      isScrollable={false}
                    >
                      {inbox}
                    </LayoutPanel>
                  }
                  content={
                    pr ? (
                      detail(pr)
                    ) : (
                      <LayoutContent>
                        <EmptyState
                          icon={<InboxIcon />}
                          title="A little perspective on every pull request."
                          description="Choose a pull request to see its changes, start a review, and keep the conversation going."
                        />
                      </LayoutContent>
                    )
                  }
                />
              )
            ) : (
              <LayoutContent padding={0}>
                <Suspense fallback={<Spinner label="Loading page…" />}>
                  {page === "live" ? (
                    <Live {...handlers} />
                  ) : page === "operations" ? (
                    <Operations {...handlers} />
                  ) : page === "loops" ? (
                    <Loops {...handlers} selected={loop} onSelect={setLoop} />
                  ) : page === "findings" ? (
                    <FindingsPage
                      selected={finding}
                      onSelect={setFinding}
                      onChoose={choose}
                    />
                  ) : page === "decisions" ? (
                    <DecisionsPage selected={chapter} onSelect={setChapter} />
                  ) : page === "scans" ? (
                    <ScansPage
                      onFindings={() => {
                        setFinding(null);
                        setPage("findings");
                      }}
                    />
                  ) : page === "repositories" ? (
                    <Repositories
                      onScans={() => setPage("scans")}
                      onSetUp={setUpWithBit}
                    />
                  ) : page === "buddy" ? (
                    <BuddyPage
                      onSettings={() => openSettings("connections")}
                      onNote={(note) => {
                        if (note.kind === "decide") {
                          setChapter(note.prId ?? null);
                          setPage("decisions");
                        } else if (note.key?.startsWith("problem:")) {
                          setPage("operations");
                        } else if (note.prId) {
                          setLoop(note.prId);
                          setPage("loops");
                        } else if (note.repo) setPage("repositories");
                        else openSettings("connections");
                      }}
                    />
                  ) : (
                    <SettingsPage />
                  )}
                </Suspense>
              </LayoutContent>
            )
          }
        />
      </AppShell>
      <Dialog
        isOpen={!!publish}
        onOpenChange={changePublishOpen}
        purpose={busy ? "required" : "info"}
        width={740}
      >
        <Layout
          header={
            <DialogHeader
              title={
                live
                  ? "Publish review to GitHub"
                  : "Simulate publishing a review"
              }
              onOpenChange={busy ? undefined : changePublishOpen}
            />
          }
          content={
            publish && (
              <LayoutContent>
                <VStack gap={4}>
                  <Text>
                    {!live
                      ? "This is a demo. Nothing will be sent to GitHub."
                      : publish.findings
                        ? "This review will be posted using your connected GitHub account."
                        : "This comment will be posted using your connected GitHub account."}
                  </Text>
                  <HStack gap={2}>
                    <Text>{publish.prId}</Text>
                    <Code>{publish.headSha.slice(0, 7)}</Code>
                  </HStack>
                  {publishError && (
                    <Banner status="error" title={publishError} />
                  )}
                  {!!publish.sensitive?.length && (
                    <>
                      <Banner
                        status="warning"
                        title={`This report contains text that looks like credentials (${publish.sensitive.join(", ")}). Codex can read files outside the review worktree, so make sure nothing private is quoted before publishing.`}
                      />
                      <Switch
                        label="I checked the report"
                        description="Publish it even though it matches a credential format."
                        value={confirmSensitive}
                        onChange={setConfirmSensitive}
                        isDisabled={busy}
                      />
                    </>
                  )}
                  <PublishPreview key={publish.id} review={publish} />
                </VStack>
              </LayoutContent>
            )
          }
          footer={
            publish && (
              <LayoutFooter>
                <HStack hAlign="end" gap={2} wrap="wrap">
                  <Button
                    label="Cancel"
                    isDisabled={busy}
                    onClick={closePublish}
                  />
                  <Button
                    label={
                      publishError
                        ? "Retry publication"
                        : live
                          ? "Publish to GitHub"
                          : "Simulate publish"
                    }
                    variant="primary"
                    icon={<Send />}
                    isLoading={busy}
                    isDisabled={
                      busy ||
                      !publishEligible ||
                      (!!publish.sensitive?.length && !confirmSensitive)
                    }
                    onClick={async () => {
                      setBusy(true);
                      setPublishError(null);
                      const result = await action(
                        "publish_review",
                        {
                          reviewId: publish.id,
                          allowSensitive: confirmSensitive,
                        },
                        { quiet: true },
                      );
                      setBusy(false);
                      if (result.ok) closePublish();
                      else setPublishError(result.error);
                    }}
                  />
                </HStack>
              </LayoutFooter>
            )
          }
        />
      </Dialog>
      <Dialog
        isOpen={!!approval && approvalOpen}
        onOpenChange={setApprovalOpen}
        purpose="info"
        width={740}
      >
        <Layout
          header={
            <DialogHeader
              title="Codex needs your approval"
              onOpenChange={setApprovalOpen}
            />
          }
          content={
            approval && (
              <LayoutContent>
                <VStack gap={4}>
                  {approvalError?.id === approval.id && (
                    <Banner status="error" title={approvalError.error} />
                  )}
                  <Text>
                    {String(
                      approval.params.reason ??
                        "Review this action before allowing the agent to continue.",
                    )}
                  </Text>
                  <Text weight="semibold">Working directory</Text>
                  <Code>
                    {String(
                      approval.params.cwd ??
                        approval.params.grantRoot ??
                        s.reviews.find((r) => r.id === approval.reviewId)
                          ?.worktree ??
                        "Not provided",
                    )}
                  </Code>
                  <CodeBlock
                    code={JSON.stringify(approval.params, null, 2)}
                    language="json"
                    isWrapped
                    width="100%"
                  />
                </VStack>
              </LayoutContent>
            )
          }
          footer={
            approval && (
              <LayoutFooter>
                <HStack hAlign="end" gap={2} wrap="wrap">
                  {[
                    [false, "Deny"],
                    [true, "Approve once"],
                  ].map(([accept, label]) => (
                    <Button
                      key={String(accept)}
                      label={String(label)}
                      variant={accept ? "primary" : "secondary"}
                      isDisabled={answeringApproval}
                      onClick={async () => {
                        if (answeringApproval) return;
                        setAnsweringApproval(true);
                        setApprovalError(null);
                        const result = await action(
                          "answer_approval",
                          {
                            id: approval.id,
                            accept,
                          },
                          { quiet: true },
                        );
                        setAnsweringApproval(false);
                        if (result.ok) setApprovalOpen(false);
                        else
                          setApprovalError({
                            id: approval.id,
                            error: result.error,
                          });
                      }}
                    />
                  ))}
                </HStack>
              </LayoutFooter>
            )
          }
        />
      </Dialog>
    </>
  );
}
