import { useEffect, useRef, useState } from "react";
import { HardDrive, RefreshCw, Trash2 } from "lucide-react";
import { useOrbit, live } from "./store";
import { activeStatuses, type StorageUsage } from "./types";
import {
  AlertDialog,
  Banner,
  Button,
  Card,
  CheckboxList,
  CheckboxListItem,
  Grid,
  Heading,
  HStack,
  List,
  ListItem,
  Text,
  VStack,
} from "@astryxdesign/core";

export function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KiB", "MiB", "GiB", "TiB"];
  let value = bytes / 1024,
    unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${value.toFixed(1)} ${units[unit]}`;
}
export function Storage() {
  const { snapshot, action, cleanup, removeWorktrees } = useOrbit();
  const { running: removing, results } = cleanup;
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const [open, setOpen] = useState(false);
  const [usage, setUsage] = useState<StorageUsage | null>(null);
  const [scanning, setScanning] = useState(false);
  const [selected, setSelected] = useState<string[]>([]);
  const [confirm, setConfirm] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const request = useRef(0);
  const refresh = async () => {
    const version = ++request.current;
    setScanning(true);
    setError(null);
    const result = await action<StorageUsage>(
      "get_storage_usage",
      {},
      { quiet: true },
    );
    if (!mounted.current || version !== request.current) return;
    setScanning(false);
    if (result.ok) {
      setUsage(result.data);
      setSelected((ids) =>
        ids.filter((id) =>
          result.data.worktrees.some((w) => w.reviewId === id && w.removable),
        ),
      );
    } else setError(result.error);
  };
  const allowed = (id: string) => {
    const r = snapshot.reviews.find((r) => r.id === id);
    return (
      !!r &&
      !!r.worktree &&
      !r.cleanupPending &&
      !activeStatuses.includes(r.status) &&
      r.status !== "publishing"
    );
  };
  const chosen = (usage?.worktrees ?? []).filter(
    (w) => selected.includes(w.reviewId) && w.removable && allowed(w.reviewId),
  );
  const estimate = chosen.reduce((n, w) => n + w.bytes, 0);
  const describe = (id: string) => {
    const r = snapshot.reviews.find((r) => r.id === id);
    return r
      ? `${r.prId} · ${new Date(r.createdAt).toLocaleString()} · ${r.id.slice(0, 8)}`
      : id;
  };
  return (
    <Card>
      <VStack gap={4}>
        <HStack hAlign="between" vAlign="center">
          <HStack gap={2} vAlign="center">
            <HardDrive aria-hidden />
            <Heading level={3}>Storage</Heading>
          </HStack>
          <Button
            label={
              open
                ? scanning
                  ? "Scanning…"
                  : "Refresh storage"
                : "Inspect storage"
            }
            variant="ghost"
            icon={<RefreshCw />}
            isLoading={scanning}
            isDisabled={scanning || removing}
            onClick={() => {
              setOpen(true);
              void refresh();
            }}
          />
        </HStack>
        <Text color="secondary">
          Remove inactive review worktrees to free space. Reports and
          conversations stay available.
        </Text>
        {removing && (
          <Text role="status">
            Removing worktrees… You can keep using Orbit while cleanup finishes.
          </Text>
        )}
        {cleanup.error && <Banner status="error" title={cleanup.error} />}
        {open && (
          <>
            {!live && (
              <Text color="secondary">
                Demo storage sizes and cleanup are simulated.
              </Text>
            )}
            {error && (
              <Banner
                status="error"
                title={error}
                endContent={
                  <Button label="Retry scan" onClick={() => void refresh()} />
                }
              />
            )}
            {usage && (
              <>
                <Grid columns={{ minWidth: 160 }} gap={4}>
                  {[
                    ["Saved state", usage.stateBytes],
                    ["Review worktrees", usage.worktreeBytes],
                    ["Unrecognized entries", usage.unrecognizedBytes],
                  ].map(([label, bytes]) => (
                    <VStack gap={1} key={label}>
                      <Text color="secondary">{label}</Text>
                      <Text type="large" weight="semibold">
                        {formatBytes(Number(bytes))}
                      </Text>
                    </VStack>
                  ))}
                </Grid>
                <Text type="supporting" color="secondary">
                  Estimated file sizes; symlinks are not followed. Source clones
                  and Codex’s own data are excluded. Unrecognized entries cannot
                  be removed here.
                </Text>
                {usage.errors.map((e, i) => (
                  <Banner
                    key={i}
                    status="error"
                    title={`Incomplete scan: ${e}`}
                  />
                ))}
                <CheckboxList
                  label="Review worktrees"
                  isLabelHidden
                  hasDividers
                  value={selected.filter(allowed)}
                  onChange={setSelected}
                  isDisabled={scanning || removing}
                >
                  {usage.worktrees.map((w) => {
                    const r = snapshot.reviews.find((r) => r.id === w.reviewId);
                    return (
                      <CheckboxListItem
                        key={w.reviewId}
                        value={w.reviewId}
                        label={describe(w.reviewId)}
                        isDisabled={!w.removable || !allowed(w.reviewId)}
                        description={
                          (w.error
                            ? `Incomplete scan: ${w.error}`
                            : r?.cleanupPending
                              ? "Removing worktree…"
                              : !allowed(w.reviewId)
                                ? "Run is active or its workspace is unavailable"
                                : "Removing this worktree ends follow-ups for this run.") +
                          (r?.cleanupError
                            ? ` Last cleanup failed: ${r.cleanupError}`
                            : "")
                        }
                        endContent={
                          <Text weight="semibold">{formatBytes(w.bytes)}</Text>
                        }
                      />
                    );
                  })}
                </CheckboxList>
                {!usage.worktrees.length && (
                  <Text>No saved review worktrees.</Text>
                )}
                {usage.unrecognized.length > 0 && (
                  <List
                    header={
                      <Heading level={4}>Unrecognized entries (kept)</Heading>
                    }
                  >
                    {usage.unrecognized.map((entry) => (
                      <ListItem
                        key={entry.name}
                        label={entry.name}
                        endContent={<Text>{formatBytes(entry.bytes)}</Text>}
                        description={
                          entry.error ? (
                            <Text role="alert">
                              Incomplete scan: {entry.error}
                            </Text>
                          ) : undefined
                        }
                      />
                    ))}
                  </List>
                )}
                <HStack>
                  <Button
                    label={`Remove worktrees (${chosen.length})`}
                    icon={<Trash2 />}
                    isDisabled={!chosen.length || scanning || removing}
                    onClick={() => setConfirm(true)}
                  />
                </HStack>
              </>
            )}
            {!!results.length && (
              <VStack gap={2} aria-live="polite">
                {results.map((r) => (
                  <VStack gap={1} key={r.reviewId}>
                    <Text role={r.error ? "alert" : undefined}>
                      {describe(r.reviewId)}:{" "}
                      {r.error || "Worktree removed; report and chat retained."}
                    </Text>
                    {r.warning && <Text role="alert">{r.warning}</Text>}
                  </VStack>
                ))}
              </VStack>
            )}
          </>
        )}
        <AlertDialog
          isOpen={confirm}
          onOpenChange={setConfirm}
          title="Remove selected worktrees"
          width={580}
          description={`Remove ${chosen.length} worktree${chosen.length === 1 ? "" : "s"}, approximately ${formatBytes(estimate)}? All files inside them, including dependencies and local changes, will be removed. Reports and chats stay readable, but these runs will no longer support follow-ups. Selected runs: ${chosen.map((w) => describe(w.reviewId)).join("; ") || "None (selection is no longer eligible)."}`}
          actionLabel="Confirm removal"
          onAction={async () => {
            if (removing || !chosen.length) return;
            const ids = chosen.map((w) => w.reviewId);
            setConfirm(false);
            setError(null);
            const result = await removeWorktrees(ids);
            if (!mounted.current) return;
            if (result.ok) {
              setSelected(
                result.data.filter((r) => r.error).map((r) => r.reviewId),
              );
              await refresh();
            }
          }}
        />
      </VStack>
    </Card>
  );
}
