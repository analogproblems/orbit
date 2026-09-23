import {
  memo,
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type ReactNode,
} from "react";
import {
  Activity,
  ArrowRight,
  ArrowUp,
  ArrowUpRight,
  Box,
  Check,
  CheckCheck,
  ChevronDown,
  ChevronRight,
  Code2,
  FileCode2,
  FolderGit2,
  GitBranch,
  GitPullRequest,
  Globe,
  Inbox,
  Loader2,
  Lock,
  MessageSquare,
  PanelLeftClose,
  Play,
  Plus,
  RefreshCw,
  Search,
  Send,
  Settings2,
  ShieldCheck,
  SlidersHorizontal,
  Sparkles,
  Square,
  Terminal,
  Trash2,
  X,
  Zap,
  AlertCircle,
  CheckCircle2,
} from "lucide-react";
import ReactMarkdown from "react-markdown";
import { parseDiff } from "./diff";
import { useOrbit, desktop } from "./store";
import {
  activeStatuses,
  statusLabels,
  latestReview,
  canPublish,
  publishTarget,
  trustedSource,
  type PullRequest,
  type Review,
  type Status,
  type Settings,
  type Repository,
  type RepositorySummary,
  type ChangedFile,
} from "./types";

type Page = "inbox" | "activity" | "repositories" | "settings";
type Tab = "overview" | "chat" | "files";
function relative(date?: string) {
  if (!date) return "Never";
  const m = Math.max(
    0,
    Math.floor((Date.now() - new Date(date).getTime()) / 60000),
  );
  return m < 1
    ? "just now"
    : m < 60
      ? `${m}m ago`
      : m < 1440
        ? `${Math.floor(m / 60)}h ago`
        : `${Math.floor(m / 1440)}d ago`;
}
function Logo({ small = false }: { small?: boolean }) {
  return (
    <img
      className={small ? "logo small" : "logo"}
      src="/orbit.svg"
      alt="Orbit"
    />
  );
}
// Every state update replaces the snapshot, so skip re-parsing unchanged text.
const Markdown = memo(function Markdown({ children }: { children: string }) {
  return (
    <div className="markdown">
      <ReactMarkdown
        components={{
          img: () => null,
          a: ({ children }) => <span className="md-link">{children}</span>,
        }}
      >
        {children}
      </ReactMarkdown>
    </div>
  );
});
function StatusBadge({ status }: { status: Status | "unreviewed" | "draft" }) {
  const running = activeStatuses.includes(status as Status);
  return (
    <span className={`status status-${status}`}>
      {running ? (
        <Loader2 size={12} className={status === "queued" ? "" : "spin"} />
      ) : status === "posted" ? (
        <CheckCheck size={12} />
      ) : status === "ready" ? (
        <CheckCircle2 size={12} />
      ) : status === "failed" ? (
        <AlertCircle size={12} />
      ) : (
        <span className="status-dot" />
      )}
      {status === "unreviewed"
        ? "Not reviewed"
        : status === "draft"
          ? "Draft"
          : statusLabels[status]}
    </span>
  );
}
function Toggle({
  checked,
  onChange,
  label,
  disabled = false,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      className={`toggle ${checked ? "on" : ""}`}
      disabled={disabled}
      onClick={() => onChange(!checked)}
    >
      <span />
    </button>
  );
}
function Empty({
  icon,
  heading,
  children,
}: {
  icon: ReactNode;
  heading: string;
  children: ReactNode;
}) {
  return (
    <div className="empty">
      <div className="empty-icon">{icon}</div>
      <h3>{heading}</h3>
      <p>{children}</p>
    </div>
  );
}
function Modal({
  title,
  children,
  onClose,
  wide = false,
}: {
  title: string;
  children: ReactNode;
  onClose: () => void;
  wide?: boolean;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  const error = useOrbit((state) => state.error);
  useEffect(() => {
    const previous = document.activeElement as HTMLElement;
    ref.current?.focus();
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeRef.current();
      if (e.key === "Tab") {
        const elements = Array.from(
          ref.current?.querySelectorAll<HTMLElement>(
            'button:not(:disabled),input,textarea,select,[tabindex="0"]',
          ) || [],
        );
        if (!elements.length) return;
        const first = elements[0],
          last = elements.at(-1);
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault();
          last?.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault();
          first.focus();
        }
      }
    };
    document.addEventListener("keydown", handler);
    return () => {
      document.removeEventListener("keydown", handler);
      previous?.focus();
    };
  }, []);
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div
        ref={ref}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className={`modal ${wide ? "wide" : ""}`}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-heading">
          <h2>{title}</h2>
          <button
            className="icon-button"
            aria-label="Close dialog"
            onClick={onClose}
          >
            <X size={19} />
          </button>
        </div>
        {error && (
          <div role="alert" className="inline-error">
            {error}
          </div>
        )}
        {children}
      </div>
    </div>
  );
}

export default function App() {
  const { snapshot: s, loaded, error, clearError, init, action } = useOrbit();
  const [page, setPage] = useState<Page>("inbox");
  const [selected, setSelected] = useState<string | null>(null);
  const [selectedReview, setSelectedReview] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("overview");
  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState("all");
  const [repoFilter, setRepoFilter] = useState("all");
  const [sidebar, setSidebar] = useState(true);
  const [publish, setPublish] = useState<Review | null>(null);
  const [confirmSensitive, setConfirmSensitive] = useState(false);
  const [busy, setBusy] = useState(false);
  const [approvalOpen, setApprovalOpen] = useState(false);
  const [, setTime] = useState(0);
  const searchRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    let off: (() => void) | undefined;
    let gone = false;
    void init().then((f) => {
      if (gone) f();
      else off = f;
    });
    return () => {
      gone = true;
      off?.();
    };
  }, [init]);
  useEffect(() => {
    const interval = setInterval(() => setTime((x) => x + 1), 15000);
    return () => clearInterval(interval);
  }, []);
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        setPage("inbox");
        searchRef.current?.focus();
      }
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, []);
  const knownPrs = [...s.prs, ...(s.historyPrs ?? [])];
  const pr = knownPrs.find((p) => p.id === selected) ?? s.prs[0];
  const review =
    (selectedReview
      ? s.reviews.find((r) => r.id === selectedReview && r.prId === pr?.id)
      : undefined) ?? latestReview(s.reviews, pr?.id ?? "");
  const visible = s.prs.filter(
    (p) =>
      (filter !== "requested" || p.reviewRequested) &&
      (filter !== "authored" || p.author === s.githubUser) &&
      (repoFilter === "all" || p.repo === repoFilter) &&
      `${p.title} ${p.repo} ${p.number} ${p.author}`
        .toLowerCase()
        .includes(search.toLowerCase()),
  );
  const active = s.reviews.filter((r) => activeStatuses.includes(r.status));
  const ready = s.reviews.filter((r) => r.status === "ready");
  const approval = s.approvals[0];
  const choose = (p: PullRequest, r?: Review) => {
    setSelected(p.id);
    setSelectedReview(r?.id ?? null);
    setTab(r ? "chat" : "overview");
    setPage("inbox");
  };
  const save = (patch: Partial<Settings>) =>
    action("save_settings", { settings: { ...s.settings, ...patch } });
  const openGithub = (url: string) =>
    desktop
      ? action("open_github", { url })
      : window.open(url, "_blank", "noopener,noreferrer");
  const closePublish = () => {
    setPublish(null);
    setConfirmSensitive(false);
  };
  const start = async () => {
    if (!pr) return;
    setBusy(true);
    setSelectedReview(null);
    await action("start_review", { prId: pr.id });
    setTab("chat");
    setBusy(false);
  };
  return (
    <div className={`app ${sidebar ? "" : "sidebar-collapsed"}`}>
      <aside className="sidebar">
        <div className="brand">
          <Logo />
          <span>
            orbit<span className="brand-period">.</span>
          </span>
          <button
            className="icon-button collapse"
            aria-label="Collapse sidebar"
            onClick={() => setSidebar(false)}
          >
            <PanelLeftClose size={16} />
          </button>
        </div>
        <div className="workspace-label">
          YOUR WORKSPACE <span>LOCAL</span>
        </div>
        <nav aria-label="Main navigation">
          <button
            className={page === "inbox" ? "nav-item selected" : "nav-item"}
            onClick={() => setPage("inbox")}
          >
            <GitPullRequest size={18} />
            <span>Pull requests</span>
            <span className="nav-count">{s.prs.length}</span>
          </button>
          <button
            className={page === "activity" ? "nav-item selected" : "nav-item"}
            onClick={() => setPage("activity")}
          >
            <Activity size={18} />
            <span>Review activity</span>
            {active.length > 0 && (
              <span className="nav-count accent">{active.length}</span>
            )}
          </button>
          <button
            className={
              page === "repositories" ? "nav-item selected" : "nav-item"
            }
            onClick={() => setPage("repositories")}
          >
            <FolderGit2 size={18} />
            <span>Repositories</span>
          </button>
        </nav>
        <div className="side-section">
          <span>RECENT REVIEWS</span>
          <span>{s.reviews.length}</span>
        </div>
        <div className="recent-list">
          {s.reviews.slice(0, 5).map((r) => {
            const p = knownPrs.find((p) => p.id === r.prId);
            return (
              <button
                key={r.id}
                onClick={() => (p ? choose(p, r) : setPage("activity"))}
                className="recent-item"
              >
                <span
                  className={`tiny-dot ${r.status === "ready" || r.status === "posted" ? "green" : activeStatuses.includes(r.status) ? "amber" : ""}`}
                />
                <div>
                  <span>{p?.title ?? r.prId}</span>
                  <small>
                    {p?.repo.split("/")[1]} #{p?.number}{" "}
                    <span>· {relative(r.updatedAt)}</span>
                  </small>
                </div>
              </button>
            );
          })}
          {!s.reviews.length && (
            <p className="side-empty">Your review chats will appear here.</p>
          )}
        </div>
        <div className="sidebar-bottom">
          <div className="connection-card">
            <div>
              <span
                className={`tiny-dot ${s.codexReady ? "green" : "amber"}`}
              />
              <span>Codex agent</span>
              <small>{s.codexReady ? "Connected" : "Disconnected"}</small>
            </div>
            <div>
              <span
                className={`tiny-dot ${s.githubUser && !s.githubError ? "green" : "amber"}`}
              />
              <span>GitHub</span>
              <small>
                {s.githubUser && !s.githubError ? "Connected" : "Not connected"}
              </small>
            </div>
          </div>
          <button
            className={page === "settings" ? "nav-item selected" : "nav-item"}
            onClick={() => setPage("settings")}
          >
            <Settings2 size={17} />
            <span>Settings</span>
            <span className="keyboard-hint">⌘ ,</span>
          </button>
          <div className="profile">
            <div className="avatar">
              {(s.githubUser ?? "O").slice(0, 2).toUpperCase()}
            </div>
            <div>
              <strong>
                {desktop
                  ? (s.githubUser ?? "Local workspace")
                  : "Demo workspace"}
              </strong>
              <small>{desktop ? "Personal workspace" : "Explore Orbit"}</small>
            </div>
            <span className="local-dot" />
          </div>
        </div>
      </aside>
      <main className="main">
        {!desktop && (
          <div className="demo-banner">
            <span>
              <span className="tiny-dot amber" />
              INTERACTIVE DEMO
            </span>
            <p>Sample pull requests. Reviews and comments are simulated.</p>
            <button onClick={() => setPage("settings")}>
              Connect your workspace <ArrowUpRight size={13} />
            </button>
          </div>
        )}
        <header className="topbar">
          <select
            className="mobile-nav"
            aria-label="Navigate workspace"
            value={page}
            onChange={(e) => setPage(e.target.value as Page)}
          >
            <option value="inbox">Pull requests</option>
            <option value="activity">Review activity</option>
            <option value="repositories">Repositories</option>
            <option value="settings">Settings</option>
          </select>
          <div className="page-heading">
            {!sidebar && (
              <button
                className="icon-button"
                aria-label="Expand sidebar"
                onClick={() => setSidebar(true)}
              >
                <PanelLeftClose size={18} />
              </button>
            )}
            <span className="breadcrumb">Workspace</span>
            <ChevronRight size={14} />
            <h1>
              {
                {
                  inbox: "Pull requests",
                  activity: "Review activity",
                  repositories: "Repositories",
                  settings: "Settings",
                }[page]
              }
            </h1>
            {page === "inbox" && (
              <span className="count-chip">{s.prs.length}</span>
            )}
          </div>
          <div className="top-actions">
            <span
              className={`automation-label ${s.settings.automatic ? "enabled" : ""}`}
            >
              <Zap size={14} />
              {s.settings.automatic ? "Autopilot on" : "Manual mode"}
            </span>
            <Toggle
              checked={s.settings.automatic}
              onChange={(v) => void save({ automatic: v })}
              label="Automatic PR reviews"
            />
            <span className="top-divider" />
            <button
              className="icon-button"
              aria-label="Open settings"
              onClick={() => setPage("settings")}
            >
              <SlidersHorizontal size={17} />
            </button>
          </div>
        </header>
        {error && (
          <div className="error-banner" role="alert">
            <AlertCircle size={17} />
            <span>{error}</span>
            <button
              className="icon-button"
              aria-label="Dismiss error"
              onClick={clearError}
            >
              <X size={16} />
            </button>
          </div>
        )}
        {approval && (
          <button
            className="approval-banner"
            onClick={() => setApprovalOpen(true)}
          >
            <ShieldCheck size={16} />
            <span>A review needs your approval to continue.</span>
            <strong>
              Review request <ArrowRight size={14} />
            </strong>
          </button>
        )}
        {!loaded ? (
          <div className="loading">
            <Loader2 className="spin" />
            Loading your workspace…
          </div>
        ) : page === "inbox" ? (
          <div className="inbox-layout">
            <section className="pr-list" aria-label="Pull request inbox">
              <div className="inbox-heading">
                <div>
                  <h2>Your review inbox</h2>
                  <p>A little less noise. A lot more context.</p>
                </div>
                <button
                  className="icon-button"
                  aria-label="Refresh pull requests"
                  disabled={s.polling}
                  onClick={() => void action("poll_now")}
                >
                  <RefreshCw size={16} className={s.polling ? "spin" : ""} />
                </button>
              </div>
              <div className="search">
                <Search size={16} />
                <input
                  ref={searchRef}
                  aria-label="Search pull requests"
                  placeholder="Search pull requests…"
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                />
                <kbd>⌘ K</kbd>
              </div>
              <div className="inbox-filters">
                <div className="segmented" aria-label="Filter pull requests">
                  {[
                    ["all", "All"],
                    ["requested", "For you"],
                    ["authored", "Authored"],
                  ].map(([value, label]) => (
                    <button
                      key={value}
                      className={filter === value ? "active" : ""}
                      onClick={() => setFilter(value)}
                    >
                      {label}
                      {value === "all" && <span>{s.prs.length}</span>}
                    </button>
                  ))}
                </div>
                <select
                  aria-label="Filter by repository"
                  value={repoFilter}
                  onChange={(e) => setRepoFilter(e.target.value)}
                >
                  <option value="all">All repos</option>
                  {[...new Set(s.prs.map((p) => p.repo))].map((repo) => (
                    <option key={repo} value={repo}>
                      {repo.split("/")[1]}
                    </option>
                  ))}
                </select>
              </div>
              <div className="list-label">
                <span>OPEN PULL REQUESTS</span>
                <span>{visible.length}</span>
              </div>
              <div className="pr-scroll">
                {visible.map((p) => {
                  const r = latestReview(s.reviews, p.id);
                  return (
                    <button
                      key={p.id}
                      className={`pr-card ${pr?.id === p.id ? "active" : ""}`}
                      onClick={() => choose(p)}
                    >
                      <div className="pr-card-meta">
                        <span>
                          <GitPullRequest size={13} />
                          {p.repo.split("/")[1]}{" "}
                          <span className="muted">#{p.number}</span>
                        </span>
                        <time>{relative(p.updatedAt)}</time>
                      </div>
                      <h3>{p.title}</h3>
                      <div className="pr-card-bottom">
                        <span className="author">
                          <span
                            className={`mini-avatar color-${p.author.length % 4}`}
                          >
                            {p.author[0].toUpperCase()}
                          </span>
                          {p.author}
                        </span>
                        <StatusBadge
                          status={
                            r?.status ?? (p.draft ? "draft" : "unreviewed")
                          }
                        />
                      </div>
                    </button>
                  );
                })}
                {!visible.length && (
                  <Empty
                    icon={<Inbox />}
                    heading={
                      search
                        ? "No matching pull requests"
                        : s.settings.repositories.length
                          ? "Your inbox is clear"
                          : "No repositories connected"
                    }
                  >
                    {s.githubError
                      ? "GitHub could not sync. Check the connection in Settings."
                      : search
                        ? "Try a different title, repository, or author."
                        : s.settings.repositories.length
                          ? "Open pull requests in your connected repositories will appear here."
                          : "Connect a repository to see its open pull requests here."}
                    {!s.settings.repositories.length && !search && (
                      <button
                        className="button primary"
                        onClick={() => setPage("repositories")}
                      >
                        <Plus size={14} />
                        Connect a repository
                      </button>
                    )}
                  </Empty>
                )}
              </div>
              <div className="poll-status">
                <span
                  className={`tiny-dot ${s.githubError ? "amber" : "green"}`}
                />
                <span>
                  {s.polling
                    ? "Syncing with GitHub…"
                    : s.githubError
                      ? "Sync needs attention"
                      : `Synced ${relative(s.lastPoll)}`}
                </span>
                <span>
                  Every{" "}
                  {s.settings.pollSeconds < 60
                    ? `${s.settings.pollSeconds}s`
                    : `${Math.round(s.settings.pollSeconds / 60)}m`}
                </span>
              </div>
            </section>
            {pr ? (
              <section className="detail">
                <div className="detail-scroll">
                  <div className="detail-breadcrumb">
                    <span>
                      <FolderGit2 size={15} />
                      {pr.repo}
                    </span>
                    <span>/</span>
                    <span>#{pr.number}</span>
                    <button
                      className="icon-button"
                      title="Open pull request on GitHub"
                      aria-label="Open pull request on GitHub"
                      onClick={() => openGithub(pr.url)}
                    >
                      <ArrowUpRight size={18} />
                    </button>
                  </div>
                  <h2 className="pr-title">{pr.title}</h2>
                  <div className="pr-subtitle">
                    <span className="open-badge">
                      <GitPullRequest size={13} />
                      {!s.prs.some((p) => p.id === pr.id)
                        ? "Saved review"
                        : pr.draft
                          ? "Draft"
                          : "Open"}
                    </span>
                    <span>
                      <strong>{pr.author}</strong> wants to merge into{" "}
                      <code>{pr.baseRef}</code>
                    </span>
                  </div>
                  <div className="branch-row">
                    <GitBranch size={14} />
                    <span title={pr.headRef}>{pr.headRef}</span>
                    <ArrowRight size={13} />
                    <code>{pr.baseRef}</code>
                    <span className="diff-stats">
                      <b>+{pr.additions.toLocaleString()}</b>
                      <i>−{pr.deletions.toLocaleString()}</i>
                    </span>
                  </div>
                  <div className="review-control">
                    <div className="review-control-icon">
                      <Sparkles size={20} />
                    </div>
                    <div>
                      <h3>
                        {review
                          ? statusLabels[review.status]
                          : "A fresh pair of eyes, on demand."}
                      </h3>
                      <p>
                        {review
                          ? `${review.model || "Codex default"} · ${review.effort || "Default"} effort · ${relative(review.updatedAt)}`
                          : "Let Codex trace the changes and find what matters."}
                      </p>
                    </div>
                    {review && activeStatuses.includes(review.status) ? (
                      <button
                        className="button secondary small-button"
                        onClick={() =>
                          void action("cancel_review", { reviewId: review.id })
                        }
                      >
                        <Square size={13} />
                        Stop
                      </button>
                    ) : (
                      <button
                        className="button primary small-button"
                        disabled={busy || !s.prs.some((p) => p.id === pr.id)}
                        onClick={() => void start()}
                      >
                        {busy ? (
                          <Loader2 size={14} className="spin" />
                        ) : (
                          <Play size={13} />
                        )}{" "}
                        {review ? "Review again" : "Start review"}
                      </button>
                    )}
                  </div>
                  {s.settings.automatic &&
                    !pr.draft &&
                    !trustedSource(pr, s.settings.trustedAuthors) &&
                    s.prs.some((p) => p.id === pr.id) &&
                    s.settings.repositories.some(
                      (r) => r.name === pr.repo && r.automatic,
                    ) && (
                      <div className="inline-error autopilot-skip">
                        <AlertCircle size={15} />
                        {pr.author &&
                        pr.headRepo?.toLowerCase() === pr.repo.toLowerCase() ? (
                          <>
                            <span>
                              Autopilot skips pull requests from {pr.author}{" "}
                              because it isn’t an owner, member, or collaborator
                              of this repository. Trust it if it opens pull
                              requests for you, like a coding agent’s bot.
                            </span>
                            <button
                              type="button"
                              className="text-button"
                              onClick={() =>
                                void save({
                                  trustedAuthors: [
                                    ...s.settings.trustedAuthors,
                                    pr.author,
                                  ],
                                })
                              }
                            >
                              Trust {pr.author}
                            </button>
                          </>
                        ) : (
                          "Autopilot skips this pull request because it comes from a fork. You can still review it manually."
                        )}
                      </div>
                    )}
                  <div className="detail-tabs" role="tablist">
                    {(
                      [
                        ["overview", "Overview", null],
                        [
                          "chat",
                          "Review chat",
                          review?.messages.filter((m) => m.role !== "system")
                            .length,
                        ],
                        ["files", "Files changed", pr.changedFiles],
                      ] as const
                    ).map(([value, label, count]) => (
                      <button
                        role="tab"
                        aria-selected={tab === value}
                        key={value}
                        className={tab === value ? "active" : ""}
                        onClick={() => setTab(value)}
                      >
                        {value === "chat" && <MessageSquare size={14} />}{" "}
                        {label}
                        {count != null && count > 0 && <span>{count}</span>}
                      </button>
                    ))}
                  </div>
                  {tab === "overview" ? (
                    <div className="overview">
                      <div className="section-heading">
                        <h3>Description</h3>
                        <span>#{pr.number}</span>
                      </div>
                      <Markdown>
                        {pr.body || "No description was provided."}
                      </Markdown>
                      {review?.report && (
                        <div className="findings-preview">
                          <div className="section-heading">
                            <h3>
                              <Sparkles size={15} /> Review is ready
                            </h3>
                            <StatusBadge status={review.status} />
                          </div>
                          <p>
                            Codex has finished reviewing this change. Read the
                            findings, ask a follow-up, or publish the review to
                            GitHub.
                          </p>
                          <button
                            className="text-button"
                            onClick={() => setTab("chat")}
                          >
                            Read the review <ArrowRight size={15} />
                          </button>
                        </div>
                      )}
                      <div className="section-heading environment-heading">
                        <h3>Review environment</h3>
                        <button
                          className="text-button"
                          onClick={() => setPage("repositories")}
                        >
                          Configure <ArrowUpRight size={13} />
                        </button>
                      </div>
                      <div className="environment-grid">
                        <div>
                          <Box size={17} />
                          <span>
                            Workspace<strong>Isolated worktree</strong>
                          </span>
                        </div>
                        <div>
                          <ShieldCheck size={17} />
                          <span>
                            Execution<strong>Codex sandbox</strong>
                          </span>
                        </div>
                        <div>
                          <Code2 size={17} />
                          <span>
                            Model
                            <strong>
                              {s.models.find(
                                (m) => m.model === s.settings.model,
                              )?.displayName || "Codex default"}
                            </strong>
                          </span>
                        </div>
                        <div>
                          <Zap size={17} />
                          <span>
                            Reasoning
                            <strong className="capitalize">
                              {s.settings.effort || "Model default"}
                            </strong>
                          </span>
                        </div>
                      </div>
                      {pr.labels.length > 0 && (
                        <div className="labels-row">
                          {pr.labels.map((l) => (
                            <span key={l}>{l}</span>
                          ))}
                        </div>
                      )}
                    </div>
                  ) : tab === "chat" ? (
                    <Chat
                      key={review?.id ?? pr.id}
                      pr={pr}
                      review={review}
                      onStart={() => void start()}
                      onPublish={setPublish}
                      onHistory={setSelectedReview}
                    />
                  ) : (
                    <Files pr={pr} />
                  )}
                </div>
                <footer className="detail-footer">
                  <span>
                    <ShieldCheck size={13} />{" "}
                    {desktop
                      ? "Local workspace · authenticated by GitHub and Codex"
                      : "Demo workspace · no external actions"}
                  </span>
                  {review?.status === "ready" && (
                    <button
                      className="text-button"
                      disabled={!canPublish(review, publishTarget(s, pr))}
                      onClick={() => setPublish(review)}
                    >
                      Publish review <ArrowUpRight size={13} />
                    </button>
                  )}
                  {review?.status === "posted" && (
                    <span className="published-label">
                      <CheckCheck size={14} />
                      {desktop ? "Comment published" : "Demo comment published"}
                    </span>
                  )}
                </footer>
              </section>
            ) : (
              <section className="detail">
                <Empty
                  icon={<GitPullRequest size={30} />}
                  heading="A home for every review"
                >
                  Connect GitHub and configure your repositories to get started.
                  <button
                    className="button primary"
                    onClick={() => setPage("settings")}
                  >
                    Open settings <ArrowRight size={15} />
                  </button>
                </Empty>
              </section>
            )}
          </div>
        ) : page === "activity" ? (
          <div className="content-page">
            <div className="page-intro">
              <span className="eyebrow">THE BIG PICTURE</span>
              <h2>Good reviews, in motion.</h2>
              <p>
                Every run, every finding, every conversation. All in one place.
              </p>
            </div>
            <div className="metrics">
              <Metric
                label="In progress"
                value={active.length}
                icon={<Activity />}
              />
              <Metric
                label="Ready to publish"
                value={ready.length}
                icon={<MessageSquare />}
              />
              <Metric
                label="Published"
                value={s.reviews.filter((r) => r.status === "posted").length}
                icon={<CheckCheck />}
              />
              <Metric
                label="Total reviews"
                value={s.reviews.length}
                icon={<GitPullRequest />}
              />
            </div>
            <div className="section-heading">
              <h3>Review history</h3>
              <span>Latest first</span>
            </div>
            <div className="activity-list">
              {s.reviews.map((r) => {
                const p = knownPrs.find((p) => p.id === r.prId);
                return (
                  <div className="activity-row" key={r.id}>
                    <div className="activity-icon">
                      <GitPullRequest size={18} />
                    </div>
                    <button disabled={!p} onClick={() => p && choose(p, r)}>
                      <strong>{p?.title ?? r.prId}</strong>
                      <span>
                        {r.prId} · {r.model || "Codex default"} ·{" "}
                        {r.automatic ? "Automatic" : "Manual"}
                      </span>
                    </button>
                    <StatusBadge status={r.status} />
                    <time>{relative(r.updatedAt)}</time>
                    {activeStatuses.includes(r.status) ? (
                      <button
                        className="icon-button"
                        aria-label={`Stop review ${r.prId}`}
                        onClick={() =>
                          void action("cancel_review", { reviewId: r.id })
                        }
                      >
                        <Square size={14} />
                      </button>
                    ) : (
                      <ChevronRight size={16} />
                    )}
                  </div>
                );
              })}
              {!s.reviews.length && (
                <Empty
                  icon={<Activity />}
                  heading="The story starts with a review"
                >
                  Start a review from your pull request inbox to see its
                  progress here.
                </Empty>
              )}
            </div>
          </div>
        ) : page === "repositories" ? (
          <Repositories />
        ) : (
          <SettingsPage />
        )}
      </main>
      {publish && (
        <Modal
          title={
            desktop
              ? "Publish review to GitHub"
              : "Simulate publishing a review"
          }
          wide
          onClose={closePublish}
        >
          <p className="modal-description">
            {desktop
              ? "This comment will be posted using your connected GitHub account."
              : "This is a demo. No comment will be sent to GitHub."}
          </p>
          <div className="publish-target">
            <GitPullRequest size={15} />
            {publish.prId}
            <code>{publish.headSha.slice(0, 7)}</code>
          </div>
          {!!publish.sensitive?.length && (
            <>
              <div role="alert" className="inline-error">
                <AlertCircle size={15} />
                This report contains text that looks like credentials (
                {publish.sensitive.join(", ")}). Codex can read files outside
                the review worktree, so make sure nothing private is quoted
                before publishing.
              </div>
              <div className="setting-row">
                <div>
                  <strong>I checked the report</strong>
                  <p>Publish it even though it matches a credential format.</p>
                </div>
                <Toggle
                  label="I checked the report"
                  checked={confirmSensitive}
                  onChange={setConfirmSensitive}
                />
              </div>
            </>
          )}
          <div className="publish-preview">
            <Markdown>{publish.report}</Markdown>
          </div>
          <div className="modal-actions">
            <button className="button secondary" onClick={closePublish}>
              Cancel
            </button>
            <button
              className="button primary"
              disabled={
                busy || (!!publish.sensitive?.length && !confirmSensitive)
              }
              onClick={async () => {
                setBusy(true);
                await action("publish_review", {
                  reviewId: publish.id,
                  allowSensitive: confirmSensitive,
                });
                setBusy(false);
                closePublish();
              }}
            >
              {busy ? (
                <Loader2 className="spin" size={15} />
              ) : (
                <Send size={15} />
              )}{" "}
              {desktop ? "Publish comment" : "Simulate publish"}
            </button>
          </div>
        </Modal>
      )}
      {approval && approvalOpen && (
        <Modal
          title="Codex needs your approval"
          wide
          onClose={() => setApprovalOpen(false)}
        >
          <p className="modal-description">
            {String(
              approval.params.reason ??
                "Review this action before allowing the agent to continue.",
            )}
          </p>
          <label>Working directory</label>
          <code className="approval-path">
            {String(
              approval.params.cwd ??
                approval.params.grantRoot ??
                s.reviews.find((r) => r.id === approval.reviewId)?.worktree ??
                "Not provided",
            )}
          </code>
          <pre className="approval-code">
            {JSON.stringify(approval.params, null, 2)}
          </pre>
          <div className="modal-actions">
            <button
              className="button secondary"
              onClick={async () => {
                await action("answer_approval", {
                  id: approval.id,
                  accept: false,
                });
                if (!useOrbit.getState().error) setApprovalOpen(false);
              }}
            >
              Deny
            </button>
            <button
              className="button primary"
              onClick={async () => {
                await action("answer_approval", {
                  id: approval.id,
                  accept: true,
                });
                if (!useOrbit.getState().error) setApprovalOpen(false);
              }}
            >
              Approve once
            </button>
          </div>
        </Modal>
      )}
    </div>
  );
}
function Metric({
  label,
  value,
  icon,
}: {
  label: string;
  value: number;
  icon: ReactNode;
}) {
  return (
    <div className="metric">
      <div>
        {icon}
        <span>{label}</span>
      </div>
      <strong>{value.toString().padStart(2, "0")}</strong>
    </div>
  );
}

function Chat({
  pr,
  review,
  onStart,
  onPublish,
  onHistory,
}: {
  pr: PullRequest;
  review?: Review;
  onStart: () => void;
  onPublish: (r: Review) => void;
  onHistory: (id: string) => void;
}) {
  const { snapshot: s, action } = useOrbit();
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const [showLogs, setShowLogs] = useState(false);
  const end = useRef<HTMLDivElement>(null);
  const running = !!review && activeStatuses.includes(review.status);
  const history = s.reviews.filter((r) => r.prId === pr.id);
  useEffect(() => {
    if (running)
      end.current?.scrollIntoView({ behavior: "smooth", block: "nearest" });
  }, [review?.messages.length, running]);
  if (!review)
    return (
      <Empty icon={<Sparkles />} heading="Meet your review partner">
        Start a review to give Codex a dedicated sandbox and a conversation
        about this pull request.
        <button className="button primary" onClick={onStart}>
          <Play size={14} />
          Start review
        </button>
      </Empty>
    );
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!text.trim() || sending) return;
    setSending(true);
    await action("send_message", { reviewId: review.id, text });
    if (!useOrbit.getState().error) setText("");
    setSending(false);
  };
  const step =
    review.status === "posted"
      ? 3
      : review.status === "ready" || review.status === "publishing"
        ? 2
        : ["reviewing", "awaitingApproval"].includes(review.status)
          ? 1
          : 0;
  return (
    <div className="chat">
      <div className="chat-toolbar">
        <div className="review-steps">
          {["Sandbox", "Review", "Publish"].map((name, i) => (
            <span key={name} className={step > i ? "done" : ""}>
              {step > i ? (
                <CheckCircle2 size={13} />
              ) : (
                <span className="step-circle" />
              )}
              {name}
              {i < 2 && <span className="step-line" />}
            </span>
          ))}
        </div>
        <button
          className={`icon-button ${showLogs ? "chosen" : ""}`}
          aria-label="Toggle execution logs"
          title="Execution logs"
          onClick={() => setShowLogs(!showLogs)}
        >
          <Terminal size={15} />
        </button>
      </div>
      {history.length > 1 && (
        <select
          aria-label="Review history"
          className="history-select"
          value={review.id}
          onChange={(e) => onHistory(e.target.value)}
        >
          {history.map((r, i) => (
            <option value={r.id} key={r.id}>
              Review {history.length - i} · {relative(r.createdAt)} ·{" "}
              {statusLabels[r.status]}
            </option>
          ))}
        </select>
      )}
      {review.error && (
        <div className="inline-error">
          <AlertCircle size={15} />
          {review.error}
        </div>
      )}
      {(review.headSha !== pr.headSha || review.baseSha !== pr.baseSha) && (
        <div className="inline-error">
          New commits are available. Start a new review before publishing.
        </div>
      )}
      <div className="messages">
        {review.messages
          .filter((m) => showLogs || m.role !== "system")
          .map((m) =>
            m.role === "system" ? (
              <details className="log-message" key={m.id}>
                <summary>
                  <Terminal size={12} />
                  {m.text.split("\n")[0]}
                </summary>
                <pre>{m.text}</pre>
              </details>
            ) : (
              <article className={`message ${m.role}`} key={m.id}>
                <div className="message-heading">
                  {m.role === "agent" ? (
                    <span className="agent-avatar">
                      <Sparkles size={13} />
                    </span>
                  ) : (
                    <span className="mini-avatar">Y</span>
                  )}
                  <strong>{m.role === "agent" ? "Codex" : "You"}</strong>
                  {m.role === "agent" && (
                    <span className="agent-label">REVIEW AGENT</span>
                  )}
                  <time>{relative(m.createdAt)}</time>
                </div>
                <Markdown>{m.text}</Markdown>
              </article>
            ),
          )}
        {running && (
          <div className="thinking">
            <span className="thinking-orbit">
              <Sparkles size={14} />
            </span>
            {statusLabels[review.status]}
            <span className="thinking-dots">•••</span>
          </div>
        )}
        {review.status === "ready" && (
          <div className="chat-completed">
            <CheckCircle2 size={15} />
            <span>Review complete. Your call from here.</span>
            <button
              className="text-button"
              disabled={!canPublish(review, publishTarget(s, pr))}
              onClick={() => onPublish(review)}
            >
              Publish findings <ArrowUpRight size={13} />
            </button>
          </div>
        )}
        <div ref={end} />
      </div>
      <form className="composer" onSubmit={(e) => void submit(e)}>
        <textarea
          aria-label="Message Codex"
          placeholder={
            running
              ? "Codex is working. You can follow up when it finishes."
              : "Ask a follow-up about this pull request…"
          }
          value={text}
          disabled={running || (!review.threadId && desktop)}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              void submit(e);
            }
          }}
        />
        <div>
          <span>
            <Box size={12} />
            {review.model || "Codex default"}
            <span>·</span>
            {review.effort || "Default"} effort
          </span>
          <button
            className="send-button"
            type="submit"
            aria-label="Send message"
            disabled={!text.trim() || running || sending}
          >
            <ArrowUp size={18} />
          </button>
        </div>
      </form>
      <p className="composer-hint">
        Your chat stays with this review.{" "}
        <span>↵ to send · Shift ↵ for a new line</span>
      </p>
    </div>
  );
}
function Files({ pr }: { pr: PullRequest }) {
  const { action } = useOrbit();
  const [files, setFiles] = useState<ChangedFile[] | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let alive = true;
    setFiles(null);
    setFailed(false);
    void action<ChangedFile[]>("get_files", { prId: pr.id }).then((result) => {
      if (alive) {
        setFiles(result ?? []);
        setFailed(!result);
      }
    });
    return () => {
      alive = false;
    };
  }, [pr.id, pr.headSha, action]);
  return (
    <div className="files">
      {files === null ? (
        <div className="loading">
          <Loader2 className="spin" size={18} />
          Loading changes…
        </div>
      ) : failed ? (
        <Empty icon={<AlertCircle />} heading="Couldn’t load changed files">
          Check the connection error above and try syncing again.
        </Empty>
      ) : (
        files.map((f) => (
          <details className="file-diff" open key={f.filename}>
            <summary>
              <FileCode2 size={15} />
              <span>{f.filename}</span>
              <b>+{f.additions}</b>
              <i>−{f.deletions}</i>
              <ChevronDown size={14} />
            </summary>
            <div className="diff-lines">
              {f.patch ? (
                parseDiff(f.patch).map((line, i) => (
                  <div key={i} className={line.kind}>
                    <span aria-label="Original line">{line.oldLine ?? ""}</span>
                    <span aria-label="Updated line">{line.newLine ?? ""}</span>
                    <code>{line.text || " "}</code>
                  </div>
                ))
              ) : (
                <p className="no-patch">
                  GitHub does not provide a preview for this file. It may be
                  binary or too large.
                </p>
              )}
            </div>
          </details>
        ))
      )}
    </div>
  );
}

function Visibility({ isPrivate }: { isPrivate: boolean }) {
  return (
    <span className={`visibility ${isPrivate ? "" : "public"}`}>
      {isPrivate ? <Lock size={11} /> : <Globe size={11} />}
      {isPrivate ? "Private" : "Public"}
    </span>
  );
}
// Loaded once per session; the refresh button picks up newly created repositories.
let repositoryCache: RepositorySummary[] | null = null;
function RepoPicker({
  value,
  onChange,
  connected,
}: {
  value: string;
  onChange: (name: string) => void;
  connected: string[];
}) {
  const { action } = useOrbit();
  const [repos, setRepos] = useState(repositoryCache);
  const [loading, setLoading] = useState(false);
  const [failed, setFailed] = useState(false);
  const [query, setQuery] = useState("");
  const load = async () => {
    setLoading(true);
    const result = await action<RepositorySummary[]>("list_repositories");
    if (result) repositoryCache = result;
    setRepos(result ?? repositoryCache ?? []);
    setFailed(!result);
    setLoading(false);
  };
  useEffect(() => {
    if (!repositoryCache) void load();
    // Load on first open only; later refreshes are explicit.
  }, []);
  const selected = repos?.find((r) => r.name === value);
  if (value)
    return (
      <>
        <div className="repo-picker-selected">
          <FolderGit2 size={15} />
          <strong>{value}</strong>
          {selected && <Visibility isPrivate={selected.private} />}
          <button
            type="button"
            className="text-button"
            onClick={() => onChange("")}
          >
            Change
          </button>
        </div>
        {selected && !selected.private && (
          <small>
            Public repository: published reviews are visible to anyone.
          </small>
        )}
      </>
    );
  const q = query.trim().toLowerCase();
  const matches = (repos ?? []).filter((r) =>
    `${r.name} ${r.description}`.toLowerCase().includes(q),
  );
  const manual =
    /^[\w.-]+\/[\w.-]+$/.test(query.trim()) &&
    !matches.some((r) => r.name.toLowerCase() === q)
      ? query.trim()
      : null;
  const pick = (name: string) => {
    onChange(name);
    setQuery("");
  };
  const limit = 50;
  return (
    <div className="repo-picker">
      <div className="repo-picker-search">
        <Search size={15} />
        <input
          aria-label="Search your repositories"
          placeholder="Search your repositories…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            // Enter picks the first match instead of submitting the form.
            if (e.key !== "Enter") return;
            e.preventDefault();
            const first = matches.find((r) => !connected.includes(r.name));
            if (first) pick(first.name);
            else if (manual) pick(manual);
          }}
        />
        <button
          type="button"
          className="icon-button"
          aria-label="Refresh repositories"
          disabled={loading}
          onClick={() => void load()}
        >
          <RefreshCw size={14} className={loading ? "spin" : ""} />
        </button>
      </div>
      <div className="repo-options">
        {repos === null ? (
          <div className="repo-options-note">
            <Loader2 className="spin" size={14} />
            Loading your repositories…
          </div>
        ) : (
          <>
            {matches.slice(0, limit).map((r) => {
              const taken = connected.includes(r.name);
              return (
                <button
                  type="button"
                  key={r.name}
                  className="repo-option"
                  disabled={taken}
                  onClick={() => pick(r.name)}
                >
                  <span>
                    <strong>{r.name}</strong>
                    {r.description && <small>{r.description}</small>}
                  </span>
                  {taken ? (
                    <em>Connected</em>
                  ) : (
                    <Visibility isPrivate={r.private} />
                  )}
                </button>
              );
            })}
            {manual && (
              <button
                type="button"
                className="repo-option"
                onClick={() => pick(manual)}
              >
                <span>
                  <strong>Use {manual}</strong>
                  <small>Not in your repository list.</small>
                </span>
              </button>
            )}
            {matches.length > limit && (
              <div className="repo-options-note">
                Showing {limit} of {matches.length}. Keep typing to narrow the
                list.
              </div>
            )}
            {!matches.length && !manual && (
              <div className="repo-options-note">
                {failed
                  ? "Couldn’t load your repositories. Type owner/name to connect one directly."
                  : "No matching repositories. Type owner/name to connect one directly."}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
function Repositories() {
  const { snapshot: s, action } = useOrbit();
  const [editing, setEditing] = useState<Repository | null>(null);
  const [original, setOriginal] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const blank: Repository = {
    name: "",
    path: "",
    automatic: false,
    setupCommand: "",
    networkAccess: false,
  };
  const save = async (e: FormEvent) => {
    e.preventDefault();
    if (!editing) return;
    setBusy(true);
    await action("save_settings", {
      settings: {
        ...s.settings,
        repositories: original
          ? s.settings.repositories.map((r) =>
              r.name === original ? editing : r,
            )
          : [...s.settings.repositories, editing],
      },
    });
    setBusy(false);
    if (!useOrbit.getState().error) {
      setEditing(null);
      void action("poll_now");
    }
  };
  return (
    <div className="content-page">
      <div className="page-intro with-action">
        <div>
          <span className="eyebrow">MAKE YOURSELF AT HOME</span>
          <h2>Every repo. Its own space.</h2>
          <p>
            Connect a checkout and define how its review environment comes
            together.
          </p>
        </div>
        <button
          className="button primary"
          onClick={() => {
            setEditing(blank);
            setOriginal(null);
          }}
        >
          <Plus size={16} />
          Add repository
        </button>
      </div>
      <div className="repository-grid">
        {s.settings.repositories.map((repo) => (
          <article className="repository-card" key={repo.name}>
            <div className="repo-card-top">
              <span className="repo-icon">
                <FolderGit2 size={22} />
              </span>
              <button
                className="icon-button"
                aria-label={`Configure ${repo.name}`}
                onClick={() => {
                  setEditing({ ...repo });
                  setOriginal(repo.name);
                }}
              >
                <SlidersHorizontal size={17} />
              </button>
            </div>
            <h3>{repo.name}</h3>
            <code>{repo.path}</code>
            <div className="repo-env">
              <Box size={14} />
              <span>
                {repo.setupCommand
                  ? "Custom environment setup"
                  : "Host tools + isolated worktree"}
              </span>
            </div>
            <div className="repo-env">
              <ShieldCheck size={14} />
              <span>
                {repo.networkAccess
                  ? "Sandbox network enabled"
                  : "Sandbox network disabled"}
              </span>
            </div>
            <div className="repo-card-footer">
              <span>
                <Zap size={14} />
                Automatic reviews
              </span>
              <Toggle
                label={`Automatic reviews for ${repo.name}`}
                checked={repo.automatic}
                onChange={(automatic) =>
                  void action("save_settings", {
                    settings: {
                      ...s.settings,
                      repositories: s.settings.repositories.map((r) =>
                        r.name === repo.name ? { ...r, automatic } : r,
                      ),
                    },
                  })
                }
              />
            </div>
          </article>
        ))}
        <button
          className="add-repository"
          onClick={() => {
            setEditing(blank);
            setOriginal(null);
          }}
        >
          <Plus size={23} />
          <strong>Connect a repository</strong>
          <span>Give your reviews a place to run</span>
        </button>
      </div>
      <div className="explanation">
        <ShieldCheck size={20} />
        <div>
          <h3>Familiar tools. A separate workspace.</h3>
          <p>
            Orbit creates a detached Git worktree at the exact PR commit.
            Reviews use your host’s installed runtimes and the setup command you
            configure, with writes confined by the Codex sandbox. The worktree
            stays available for follow-up conversations.
          </p>
        </div>
      </div>
      {editing && (
        <Modal
          title={original ? "Repository settings" : "Connect a repository"}
          onClose={() => setEditing(null)}
        >
          <form onSubmit={(e) => void save(e)}>
            <div className="field">
              GitHub repository
              <RepoPicker
                value={editing.name}
                onChange={(name) => setEditing({ ...editing, name })}
                connected={s.settings.repositories
                  .map((r) => r.name)
                  .filter((name) => name !== original)}
              />
            </div>
            <label>
              Local checkout path
              <input
                required
                placeholder="/Users/you/Projects/repository"
                value={editing.path}
                onChange={(e) =>
                  setEditing({ ...editing, path: e.target.value })
                }
              />
              <small>
                Use an absolute path to an existing clone with a matching
                origin.
              </small>
            </label>
            <label>
              Environment setup <span className="muted">optional</span>
              <textarea
                placeholder="e.g. npm ci --ignore-scripts"
                value={editing.setupCommand}
                onChange={(e) =>
                  setEditing({ ...editing, setupCommand: e.target.value })
                }
              />
              <small>
                Runs in the new worktree, inside the Codex sandbox. Uses /bin/sh
                on Unix or PowerShell on Windows. Five-minute timeout. Setup can
                run code from the pull request, such as package install scripts;
                prefer options like --ignore-scripts.
              </small>
            </label>
            <div className="setting-row">
              <div>
                <strong>Allow sandbox network access</strong>
                <p>
                  Enable when setup needs to download dependencies. Setup and
                  the review agent can then reach the network, including code
                  from the pull request.
                </p>
              </div>
              <Toggle
                checked={editing.networkAccess}
                onChange={(networkAccess) =>
                  setEditing({ ...editing, networkAccess })
                }
                label="Allow sandbox network access"
              />
            </div>
            <div className="setting-row">
              <div>
                <strong>Include in automatic reviews</strong>
                <p>Runs when workspace autopilot is enabled.</p>
              </div>
              <Toggle
                checked={editing.automatic}
                onChange={(automatic) => setEditing({ ...editing, automatic })}
                label="Include repository in automatic reviews"
              />
            </div>
            <div className="modal-actions">
              {original && (
                <button
                  type="button"
                  className="button danger-quiet"
                  onClick={async () => {
                    await action("save_settings", {
                      settings: {
                        ...s.settings,
                        repositories: s.settings.repositories.filter(
                          (r) => r.name !== original,
                        ),
                      },
                    });
                    if (!useOrbit.getState().error) {
                      setEditing(null);
                      void action("poll_now");
                    }
                  }}
                >
                  <Trash2 size={14} />
                  Disconnect
                </button>
              )}
              <button
                className="button primary"
                disabled={busy || !editing.name}
                type="submit"
              >
                {busy ? (
                  <Loader2 size={14} className="spin" />
                ) : (
                  <Check size={14} />
                )}
                Save repository
              </button>
            </div>
          </form>
        </Modal>
      )}
    </div>
  );
}
function SettingsPage() {
  const { snapshot: s, action } = useOrbit();
  const [draft, setDraft] = useState(s.settings);
  const [saved, setSaved] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [newLogin, setNewLogin] = useState("");
  const addLogin = () => {
    const login = newLogin.trim();
    if (
      login &&
      !draft.trustedAuthors.some((a) => a.toLowerCase() === login.toLowerCase())
    )
      setDraft({ ...draft, trustedAuthors: [...draft.trustedAuthors, login] });
    setNewLogin("");
  };
  // Repositories are edited on their own page and never saved from this form.
  const formKey = (settings: Settings) =>
    JSON.stringify({ ...settings, repositories: undefined });
  const settingsKey = formKey(s.settings);
  const baseline = useRef(settingsKey);
  useEffect(() => {
    // Adopt changes made elsewhere only while the form has no unsaved edits.
    // The updater runs later, so compare against the baseline captured now.
    const previous = baseline.current;
    baseline.current = settingsKey;
    setDraft((current) =>
      formKey(current) === previous
        ? { ...(JSON.parse(settingsKey) as Settings), repositories: [] }
        : current,
    );
  }, [settingsKey]);
  const model =
    s.models.find((m) => m.model === draft.model) ||
    s.models.find((m) => m.isDefault);
  const save = async (e: FormEvent) => {
    e.preventDefault();
    await action("save_settings", {
      settings: { ...draft, repositories: s.settings.repositories },
    });
    if (!useOrbit.getState().error) {
      setSaved(true);
      setTimeout(() => setSaved(false), 2500);
    }
  };
  return (
    <div className="content-page settings-page">
      <div className="page-intro">
        <span className="eyebrow">YOUR AGENT. YOUR RULES.</span>
        <h2>A review flow that fits.</h2>
        <p>
          Set the pace, pick the model, and choose how hands-on you want to be.
        </p>
      </div>
      <section className="settings-section">
        <div className="section-heading">
          <h3>Connections</h3>
          <button
            disabled={connecting}
            className="text-button"
            onClick={async () => {
              setConnecting(true);
              await action("connect_services");
              setConnecting(false);
            }}
          >
            <RefreshCw size={14} className={connecting ? "spin" : ""} />
            {connecting ? "Connecting…" : "Reconnect"}
          </button>
        </div>
        <div className="connection-row">
          <span className="connection-icon">
            <Code2 size={20} />
          </span>
          <div>
            <strong>Codex CLI</strong>
            <p>
              {s.codexError ||
                "Uses your existing Codex login and available models."}
            </p>
          </div>
          <span className={`connection-status ${s.codexReady ? "good" : ""}`}>
            {s.codexReady ? "Connected" : "Not connected"}
          </span>
        </div>
        <div className="connection-row">
          <span className="connection-icon">
            <GitBranch size={20} />
          </span>
          <div>
            <strong>GitHub CLI</strong>
            <p>
              {s.githubError ||
                (s.githubUser
                  ? `Connected as @${s.githubUser}`
                  : "Run gh auth login in your terminal to connect.")}
            </p>
          </div>
          <span
            className={`connection-status ${s.githubUser && !s.githubError ? "good" : ""}`}
          >
            {s.githubUser && !s.githubError ? "Connected" : "Not connected"}
          </span>
        </div>
        {!desktop && (
          <div className="setup-note">
            <Terminal size={16} />
            <div>
              <strong>Ready for your real repositories?</strong>
              <p>
                Run <code>npm run desktop</code> in this project. The native app
                connects to <code>gh</code> and <code>codex</code> installed on
                your computer.
              </p>
              <button
                type="button"
                className="text-button"
                onClick={() => void action("reset_demo")}
              >
                Reset sample workspace
              </button>
            </div>
          </div>
        )}
      </section>
      <form onSubmit={(e) => void save(e)}>
        <section className="settings-section">
          <div className="section-heading">
            <h3>Review agent</h3>
            <Sparkles size={15} />
          </div>
          <div className="form-grid">
            <label>
              Model
              <select
                value={draft.model}
                onChange={(e) =>
                  setDraft({ ...draft, model: e.target.value, effort: "" })
                }
              >
                <option value="">Codex default</option>
                {s.models.map((m) => (
                  <option key={m.id} value={m.model}>
                    {m.displayName}
                  </option>
                ))}
              </select>
              <small>
                Available models are loaded from your Codex account.
              </small>
            </label>
            <label>
              Reasoning effort
              <select
                value={draft.effort}
                onChange={(e) => setDraft({ ...draft, effort: e.target.value })}
              >
                <option value="">Model default</option>
                {model?.supportedReasoningEfforts.map((e) => (
                  <option key={e.reasoningEffort} value={e.reasoningEffort}>
                    {e.reasoningEffort[0].toUpperCase() +
                      e.reasoningEffort.slice(1)}
                  </option>
                ))}
              </select>
              <small>More effort gives the agent more time to reason.</small>
            </label>
          </div>
        </section>
        <section className="settings-section">
          <div className="section-heading">
            <h3>Automation</h3>
            <Zap size={15} />
          </div>
          <div className="setting-row">
            <div>
              <strong>Automatically review new pull requests</strong>
              <p>
                Create a sandbox and chat for each new commit in enabled
                repositories. Drafts are skipped, as are pull requests from
                forks or from authors who aren’t owners, members, collaborators,
                or trusted accounts.
              </p>
            </div>
            <Toggle
              label="Automatically review new pull requests"
              checked={draft.automatic}
              onChange={(automatic) => setDraft({ ...draft, automatic })}
            />
          </div>
          <div className="setting-row">
            <div>
              <strong>Automatically publish findings</strong>
              <p>
                Post completed automatic reviews as PR comments using your
                GitHub account. Only pull requests opened from the repository
                itself by its owners, members, collaborators, or trusted
                accounts are published automatically, and never when a report
                looks like it contains credentials. Everything else waits for
                you to publish it.
              </p>
            </div>
            <Toggle
              label="Automatically publish findings"
              checked={draft.autoPost}
              onChange={(autoPost) => setDraft({ ...draft, autoPost })}
            />
          </div>
          <div className="setting-row">
            <div>
              <strong>Trusted accounts</strong>
              <p>
                Pull requests these accounts open from a branch in the
                repository are reviewed and published automatically, like your
                own. Only add bots that act for you: anyone who can direct a
                trusted bot can get its pull requests reviewed automatically.
              </p>
              <div className="trusted-list">
                {draft.trustedAuthors.map((login) => (
                  <span className="trusted-chip" key={login}>
                    {login}
                    <button
                      type="button"
                      aria-label={`Stop trusting ${login}`}
                      onClick={() =>
                        setDraft({
                          ...draft,
                          trustedAuthors: draft.trustedAuthors.filter(
                            (a) => a !== login,
                          ),
                        })
                      }
                    >
                      <X size={12} />
                    </button>
                  </span>
                ))}
                <span className="trusted-add">
                  <input
                    aria-label="Add a trusted account"
                    placeholder="e.g. claude[bot]"
                    value={newLogin}
                    onChange={(e) => setNewLogin(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key !== "Enter") return;
                      e.preventDefault();
                      addLogin();
                    }}
                  />
                  <button
                    type="button"
                    className="button secondary"
                    disabled={!newLogin.trim()}
                    onClick={addLogin}
                  >
                    Add
                  </button>
                </span>
              </div>
            </div>
          </div>
          <div className="form-grid">
            <label>
              Concurrent reviews
              <select
                value={draft.concurrency}
                onChange={(e) =>
                  setDraft({ ...draft, concurrency: Number(e.target.value) })
                }
              >
                {[1, 2, 3, 4].map((n) => (
                  <option value={n} key={n}>
                    {n} {n === 1 ? "review" : "reviews"} at a time
                  </option>
                ))}
              </select>
            </label>
            <label>
              Poll interval
              <select
                value={draft.pollSeconds}
                onChange={(e) =>
                  setDraft({ ...draft, pollSeconds: Number(e.target.value) })
                }
              >
                {[30, 60, 120, 300, 600, 1800, 3600].map((n) => (
                  <option value={n} key={n}>
                    {n < 60
                      ? `${n} seconds`
                      : `${n / 60} ${n === 60 ? "minute" : "minutes"}`}
                  </option>
                ))}
              </select>
            </label>
          </div>
        </section>
        <div className="settings-footer">
          <span>
            <ShieldCheck size={14} />
            Settings are saved on this device.
          </span>
          <button className="button primary" type="submit">
            {saved ? <Check size={15} /> : <CheckCheck size={15} />}{" "}
            {saved ? "Saved" : "Save changes"}
          </button>
        </div>
      </form>
    </div>
  );
}
