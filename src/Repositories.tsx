import {
  useCallback,
  useEffect,
  useId,
  useMemo,
  useState,
  type FormEvent,
} from "react";
import {
  Banner,
  Button,
  Code,
  Dialog,
  DialogHeader,
  EmptyState,
  FormLayout,
  Heading,
  HStack,
  IconButton,
  Layout,
  LayoutContent,
  LayoutFooter,
  List,
  ListItem,
  Selector,
  Spinner,
  StatusDot,
  Switch,
  Text,
  TextArea,
  TextInput,
  Token,
  Typeahead,
  VStack,
} from "@astryxdesign/core";
import {
  FolderGit2,
  FolderOpen,
  Plus,
  RefreshCw,
  ShieldCheck,
  SlidersHorizontal,
  Trash2,
} from "lucide-react";
import { desktop, useOrbit } from "./store";
import {
  sameRepo,
  type CloneUpdate,
  type Provider,
  type Repository,
  type RepositorySummary,
} from "./types";
import { SaveFeedback } from "./SaveFeedback";
import { ScanDialog } from "./Scans";

// Loaded once per session; explicit refresh picks up new GitHub repositories.
let repositoryCache: RepositorySummary[] | null = null;
function RepoPicker({
  value,
  onChange,
  connected,
  isDisabled,
}: {
  value: string;
  onChange: (name: string) => void;
  connected: string[];
  isDisabled: boolean;
}) {
  const action = useOrbit((s) => s.action);
  const [repos, setRepos] = useState(repositoryCache);
  const [refreshing, setRefreshing] = useState(false);
  const [failed, setFailed] = useState(false);
  const [query, setQuery] = useState("");
  const fetchRepos = useCallback(
    () =>
      action<RepositorySummary[]>(
        "list_repositories",
        {},
        { quiet: true },
      ).then((result) => {
        if (result.ok) repositoryCache = result.data;
        setRepos(result.ok ? result.data : (repositoryCache ?? []));
        setFailed(!result.ok);
      }),
    [action],
  );
  useEffect(() => {
    if (!repositoryCache) void fetchRepos();
  }, [fetchRepos]);
  const source = useMemo(() => {
    const taken = (name: string) =>
      connected.some((c) => c.toLowerCase() === name.toLowerCase());
    const search = (query: string) => {
      const q = query.trim().toLowerCase();
      const matches = (repos ?? []).filter((r) =>
        `${r.name} ${r.description}`.toLowerCase().includes(q),
      );
      const items = matches
        .filter((r) => !taken(r.name))
        .map((r) => ({
          id: r.name,
          label: r.name,
          auxiliaryData: {
            description: r.description,
            private: r.private,
            manual: false,
          },
        }));
      if (
        /^[\w.-]+\/[\w.-]+$/.test(query.trim()) &&
        !taken(query.trim()) &&
        !matches.some((r) => r.name.toLowerCase() === q)
      )
        items.push({
          id: query.trim(),
          label: query.trim(),
          auxiliaryData: {
            description: "Not in your repository list. Connect directly.",
            private: false,
            manual: true,
          },
        });
      return items;
    };
    return { search, bootstrap: () => search("") };
  }, [repos, connected]);
  const selected = repos?.find(
    (r) => r.name.toLowerCase() === value.toLowerCase(),
  );
  const matchCount = source.search(query).length;
  return (
    <VStack gap={2}>
      {value ? (
        <VStack gap={2}>
          <HStack gap={2} vAlign="center" hAlign="between">
            <Text weight="semibold">{value}</Text>
            {selected && (
              <Token label={selected.private ? "Private" : "Public"} />
            )}
            <Button
              label="Change"
              variant="ghost"
              isDisabled={isDisabled}
              onClick={() => {
                setQuery("");
                onChange("");
              }}
            />
          </HStack>
          {selected && !selected.private && (
            <Text type="supporting" color="secondary">
              Public repository: published reviews are visible to anyone.
            </Text>
          )}
        </VStack>
      ) : (
        <HStack gap={2} vAlign="end">
          <Typeahead
            label="Search your repositories"
            description="Choose a GitHub repository, or type owner/name to connect directly. Connected repositories are omitted."
            placeholder="Search your repositories…"
            width="100%"
            searchSource={source}
            value={null}
            onChangeQuery={setQuery}
            onChange={(item) => {
              if (item) onChange(item.id);
            }}
            hasEntriesOnFocus
            minQueryLength={0}
            maxMenuItems={50}
            debounceMs={0}
            isDisabled={isDisabled}
            renderItem={(item) => (
              <VStack gap={1}>
                <Text weight="semibold">
                  {item.auxiliaryData.manual ? `Use ${item.label}` : item.label}
                </Text>
                <Text color="secondary" type="supporting">
                  {item.auxiliaryData.description}
                </Text>
                {!item.auxiliaryData.manual && (
                  <Token
                    size="sm"
                    label={item.auxiliaryData.private ? "Private" : "Public"}
                  />
                )}
              </VStack>
            )}
          />
          <IconButton
            label="Refresh repositories"
            icon={<RefreshCw />}
            isLoading={refreshing}
            isDisabled={isDisabled || refreshing}
            onClick={async () => {
              setRefreshing(true);
              await fetchRepos();
              setRefreshing(false);
            }}
          />
        </HStack>
      )}
      {!value && matchCount > 50 && (
        <Text role="status" type="supporting" color="secondary">
          Showing 50 of {matchCount} matches. Keep typing to narrow the results.
        </Text>
      )}
      {repos === null && <Spinner label="Loading your repositories…" />}
      {failed && (
        <Banner
          status="warning"
          title="Couldn’t load your repositories. Type owner/name to connect one directly."
        />
      )}
    </VStack>
  );
}
const cloneDot = {
  updated: "success",
  current: "success",
  skipped: "warning",
  failed: "error",
} as const;
function CloneUpdateStatus({ update }: { update?: CloneUpdate }) {
  return (
    <HStack gap={2} vAlign="center">
      <StatusDot
        variant={update ? cloneDot[update.state] : "neutral"}
        label="Clone update"
      />
      <Text type="supporting" color="secondary">
        {update ? update.message : "Orbit updates this clone on the next sync."}
      </Text>
    </HStack>
  );
}
export function Repositories({ onScans }: { onScans?: () => void } = {}) {
  const formId = useId();
  const [scanning, setScanning] = useState<string | null>(null);
  const { snapshot: s, action, mutate, saves } = useOrbit();
  const [repoError, setRepoError] = useState<string | null>(null);
  const [editing, setEditing] = useState<Repository | null>(null);
  const [original, setOriginal] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const add = () => {
    setRepoError(null);
    setOriginal(null);
    setEditing({
      name: "",
      path: "",
      automatic: false,
      setupCommand: "",
      networkAccess: false,
      roundLimit: 4,
      approvalMinutes: 0,
      keepUpdated: false,
    });
  };
  const save = async (e: FormEvent) => {
    e.preventDefault();
    if (!editing || busy) return;
    if (!editing.path.trim()) {
      setRepoError("Enter a local checkout path.");
      return;
    }
    setBusy(true);
    setRepoError(null);
    const result = await mutate(
      "save_repository",
      { repository: editing, original },
      `repository:${original ?? editing.name}`,
    );
    setBusy(false);
    if (result.ok) {
      setEditing(null);
      void action("poll_now");
    } else setRepoError(result.error);
  };
  const claudeReviewer = editing?.provider === "claude";
  const close = (open: boolean) => {
    if (!open && !busy) setEditing(null);
  };
  return (
    <VStack gap={6} padding={6} maxWidth={960} className="content-page">
      <HStack gap={4} hAlign="between" vAlign="start" wrap="wrap">
        <VStack gap={2}>
          <Heading level={2}>Every repo. Its own space.</Heading>
          <Text color="secondary">
            Connect a checkout and define how its review environment comes
            together.
          </Text>
        </VStack>
        <Button
          label="Add repository"
          variant="primary"
          icon={<Plus />}
          onClick={add}
        />
      </HStack>
      <List hasDividers>
        {s.settings.repositories.map((repo) => (
          <ListItem
            key={repo.name}
            label={repo.name}
            startContent={<FolderGit2 />}
            endContent={
              <HStack gap={2}>
                <IconButton
                  label={`Run security scan of ${repo.name}`}
                  tooltip="Run security scan"
                  icon={<ShieldCheck />}
                  onClick={() => setScanning(repo.name)}
                />
                <IconButton
                  label={`Configure ${repo.name}`}
                  icon={<SlidersHorizontal />}
                  onClick={() => {
                    setRepoError(null);
                    setEditing({ ...repo });
                    setOriginal(repo.name);
                  }}
                />
              </HStack>
            }
            description={
              <VStack gap={3} paddingBlock={2}>
                <Code>{repo.path}</Code>
                {repo.keepUpdated && (
                  <CloneUpdateStatus
                    update={s.cloneUpdates?.find((u) =>
                      sameRepo(u.repo, repo.name),
                    )}
                  />
                )}
                <Text color="secondary">
                  {repo.provider === "claude"
                    ? "Reviewed by Claude · Read-only worktree · No network"
                    : `Reviewed by Codex · ${
                        repo.setupCommand
                          ? "Custom environment setup"
                          : "Host tools + isolated worktree"
                      } · ${
                        repo.networkAccess
                          ? "Sandbox network enabled"
                          : "Sandbox network disabled"
                      }`}
                </Text>
                <Switch
                  label={`Automatic reviews for ${repo.name}`}
                  value={repo.automatic}
                  isDisabled={
                    saves[`repository:${repo.name}`]?.state === "saving"
                  }
                  onChange={(automatic) =>
                    void mutate(
                      "set_repository_automatic",
                      { name: repo.name, automatic },
                      `repository:${repo.name}`,
                    )
                  }
                />
                <SaveFeedback saveKey={`repository:${repo.name}`} />
              </VStack>
            }
          />
        ))}
      </List>
      {!s.settings.repositories.length && (
        <EmptyState
          title="No repositories connected"
          description="Give your reviews a place to run."
          icon={<FolderGit2 />}
          actions={
            <Button
              label="Connect a repository"
              icon={<Plus />}
              onClick={add}
            />
          }
        />
      )}
      <Banner
        status="info"
        title="Familiar tools. A separate workspace."
        description="Orbit creates a detached Git worktree at the exact PR commit. Reviews use your host’s installed runtimes and the setup command you configure, with writes confined by the Codex sandbox. The worktree stays available for follow-up conversations."
      />
      <ScanDialog
        isOpen={!!scanning}
        onOpenChange={(open) => !open && setScanning(null)}
        repo={scanning ?? undefined}
        onStarted={onScans}
      />
      <Dialog
        isOpen={!!editing}
        onOpenChange={close}
        purpose={busy ? "required" : "form"}
        width={620}
      >
        <Layout
          header={
            <DialogHeader
              title={original ? "Repository settings" : "Connect a repository"}
              onOpenChange={busy ? undefined : close}
            />
          }
          content={
            editing && (
              <LayoutContent>
                <VStack
                  as="form"
                  id={formId}
                  gap={4}
                  onSubmit={(e) => void save(e)}
                >
                  {repoError && <Banner status="error" title={repoError} />}
                  <RepoPicker
                    value={editing.name}
                    onChange={(name) => setEditing({ ...editing, name })}
                    connected={s.settings.repositories
                      .map((r) => r.name)
                      .filter((name) => name !== original)}
                    isDisabled={busy}
                  />
                  <FormLayout>
                    <HStack gap={2} vAlign="end">
                      <TextInput
                        label="Local checkout path"
                        width="100%"
                        isRequired
                        placeholder="/Users/you/Projects/repository"
                        description="Use an absolute path to an existing clone whose origin is this repository. It does not need to be up to date: reviews fetch the commits they need."
                        value={editing.path}
                        isDisabled={busy}
                        onChange={(path) => setEditing({ ...editing, path })}
                      />
                      {desktop && (
                        <Button
                          label="Browse…"
                          icon={<FolderOpen />}
                          isDisabled={busy}
                          onClick={async () => {
                            const picked = await action<string | null>(
                              "choose_checkout",
                              { current: editing.path.trim() || null },
                              { quiet: true },
                            );
                            if (!picked.ok) setRepoError(picked.error);
                            else if (picked.data)
                              setEditing((e) =>
                                e ? { ...e, path: picked.data as string } : e,
                              );
                          }}
                        />
                      )}
                    </HStack>
                    <Switch
                      label="Keep this clone up to date"
                      value={editing.keepUpdated ?? false}
                      isDisabled={busy}
                      onChange={(keepUpdated) =>
                        setEditing({ ...editing, keepUpdated })
                      }
                      description="On each sync, Orbit fetches from GitHub and fast-forwards the checked-out branch when it is behind. It never touches a branch with uncommitted changes or local commits; the repository shows why instead."
                    />
                    <Selector
                      label="Reviewer"
                      value={editing.provider ?? "codex"}
                      isDisabled={busy}
                      onChange={(provider) =>
                        setEditing({
                          ...editing,
                          provider: provider as Provider,
                        })
                      }
                      options={[
                        { value: "codex", label: "Codex" },
                        {
                          value: "claude",
                          label: s.claude?.ready
                            ? "Claude"
                            : "Claude (not connected)",
                        },
                      ]}
                      description={
                        claudeReviewer
                          ? "Claude reviews with its own code review rules, using the model and effort in Settings. It can read files and run a short list of Git and text commands, all inside Claude's sandbox with no network access, and declines anything else on its own, so it never waits for your approval. Like Codex's sandbox, this is not a full read boundary, and a Git diff driver configured in your clone can still run inside it and write to the worktree."
                          : "Codex reviews in its sandbox, using the model and effort in Settings."
                      }
                    />
                    {(!claudeReviewer || editing.setupCommand) && (
                      <TextArea
                        label="Environment setup"
                        isOptional
                        placeholder="e.g. npm ci --ignore-scripts"
                        value={editing.setupCommand}
                        isDisabled={busy}
                        onChange={(setupCommand) =>
                          setEditing({ ...editing, setupCommand })
                        }
                        description="Runs in the new worktree, inside the Codex sandbox. Uses /bin/sh on Unix or PowerShell on Windows. Five-minute timeout. Setup can run code from the pull request, such as package install scripts; prefer options like --ignore-scripts."
                        status={
                          claudeReviewer
                            ? {
                                type: "error",
                                message:
                                  "Claude reviews can't run a setup command. Clear it to save.",
                              }
                            : undefined
                        }
                      />
                    )}
                    {!claudeReviewer && (
                      <Switch
                        label="Allow sandbox network access"
                        value={editing.networkAccess}
                        isDisabled={busy}
                        onChange={(networkAccess) =>
                          setEditing({ ...editing, networkAccess })
                        }
                        description="Enable when setup needs to download dependencies. Setup and the review agent can then reach the network, including code from the pull request."
                      />
                    )}
                    <Selector
                      label="Automatic rounds per pull request"
                      value={String(editing.roundLimit ?? 4)}
                      isDisabled={busy}
                      onChange={(v) =>
                        setEditing({ ...editing, roundLimit: Number(v) })
                      }
                      options={[1, 2, 3, 4, 5, 6, 8, 10].map((n) => ({
                        value: String(n),
                        label: `${n} ${n === 1 ? "round" : "rounds"}`,
                      }))}
                      description="After this many automatic reviews of a pull request, the loop stops and needs you. Review again always works."
                    />
                    {!claudeReviewer && (
                      <Selector
                        label="Approval time limit for automatic reviews"
                        value={String(editing.approvalMinutes ?? 0)}
                        isDisabled={busy}
                        onChange={(v) =>
                          setEditing({ ...editing, approvalMinutes: Number(v) })
                        }
                        options={[0, 5, 15, 30, 60, 120].map((n) => ({
                          value: String(n),
                          label: n
                            ? `Decline after ${n} minutes`
                            : "Wait for me",
                        }))}
                        description="When an automatic review asks to run a command or change files and no one answers in time, Orbit declines the request and notes it in the report. Declining grants nothing. Manual reviews always wait."
                      />
                    )}
                    <Switch
                      label="Include repository in automatic reviews"
                      value={editing.automatic}
                      isDisabled={busy}
                      onChange={(automatic) =>
                        setEditing({ ...editing, automatic })
                      }
                      description="Runs when workspace autopilot is enabled."
                    />
                  </FormLayout>
                </VStack>
              </LayoutContent>
            )
          }
          footer={
            editing && (
              <LayoutFooter>
                <HStack hAlign="end" gap={2} wrap="wrap">
                  {original && (
                    <Button
                      label="Disconnect"
                      variant="destructive"
                      icon={<Trash2 />}
                      isDisabled={busy}
                      onClick={async () => {
                        setBusy(true);
                        const result = await mutate(
                          "disconnect_repository",
                          { name: original },
                          `repository:${original}`,
                        );
                        setBusy(false);
                        if (result.ok) {
                          setEditing(null);
                          void action("poll_now");
                        } else setRepoError(result.error);
                      }}
                    />
                  )}
                  <Button
                    label="Save repository"
                    variant="primary"
                    isLoading={busy}
                    isDisabled={busy || !editing.name}
                    type="submit"
                    form={formId}
                  />
                </HStack>
              </LayoutFooter>
            )
          }
        />
      </Dialog>
    </VStack>
  );
}
