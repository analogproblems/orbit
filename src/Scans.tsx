import { useEffect, useState } from "react";
import {
  AlertDialog,
  Banner,
  Button,
  Dialog,
  DialogHeader,
  EmptyState,
  Heading,
  HStack,
  IconButton,
  Layout,
  LayoutContent,
  LayoutFooter,
  List,
  ListItem,
  ProgressBar,
  RadioList,
  RadioListItem,
  Selector,
  Spinner,
  Text,
  Timestamp,
  Token,
  VStack,
} from "@astryxdesign/core";
import { Bug, FileText, ShieldCheck, Square, Trash2 } from "lucide-react";
import { useOrbit } from "./store";
import {
  scanActive,
  type ScanMode,
  type ScanPluginStatus,
  type ScanProgress,
  type ScanStatus,
  type SecurityScan,
} from "./types";
import { priorityColors } from "./Findings";
import { formatBytes } from "./Storage";
import { SafeMarkdown } from "./SafeMarkdown";
import { useNow } from "./Usage";

export const scanStatusLabels: Record<ScanStatus, string> = {
  preparing: "Preparing",
  running: "Running",
  cancelling: "Stopping",
  importing: "Importing findings",
  completed: "Completed",
  failed: "Failed",
  cancelled: "Cancelled",
  interrupted: "Interrupted",
};
const scanStatusColors: Record<
  ScanStatus,
  "blue" | "green" | "red" | "gray" | "orange"
> = {
  preparing: "blue",
  running: "blue",
  cancelling: "orange",
  importing: "blue",
  completed: "green",
  failed: "red",
  cancelled: "gray",
  interrupted: "orange",
};
export const scanModeLabels: Record<ScanMode, string> = {
  standard: "Standard",
  deep: "Deep",
};
const phaseLabels: Record<string, string> = {
  preflight: "Checking the setup",
  threat_model: "Building a threat model",
  discovery: "Looking for vulnerabilities",
  validation: "Checking candidate findings",
  attack_path: "Tracing attack paths",
  reporting: "Writing the report",
};

/** What the scan is doing, in words. */
export function progressLabel(scan: SecurityScan): string {
  if (scan.status === "preparing")
    return "Copying the repository at its latest commit";
  if (scan.status === "importing") return "Importing findings";
  if (scan.status === "cancelling")
    return scan.error
      ? "Not confirmed stopped yet"
      : "Stopping the Codex Security scan";
  const p: ScanProgress | null | undefined = scan.progress;
  if (!p?.phase) return "Starting the Codex Security scan";
  const phase = phaseLabels[p.phase] ?? p.phase.replaceAll("_", " ");
  const counted = p.total
    ? ` · ${p.completed ?? 0} of ${p.total} ${p.unit ?? "items"}`
    : "";
  const found =
    p.findings != null
      ? ` · ${p.findings} ${p.findings === 1 ? "finding" : "findings"} so far`
      : "";
  return `${phase}${counted}${found}`;
}
/** How long a scan ran, or has run so far. */
export function scanDuration(scan: SecurityScan, now: number): string {
  const end = scan.finishedAt ? Date.parse(scan.finishedAt) : now;
  const minutes = Math.max(
    0,
    Math.floor((end - Date.parse(scan.createdAt)) / 60_000),
  );
  if (minutes < 1) return "under a minute";
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  return minutes % 60 ? `${hours} h ${minutes % 60} min` : `${hours} h`;
}
export function severityCounts(scan: SecurityScan): number[] {
  return [0, 1, 2, 3].map(
    (p) => scan.findings.filter((f) => f.priority === p).length,
  );
}
function daybreakWarning(scan: SecurityScan): string | null {
  if (scan.mode !== "deep" || !scan.daybreak || scan.daybreak === "granted")
    return null;
  return scan.daybreak === "not_granted"
    ? "Your ChatGPT account does not have Daybreak access, so some protected results may not be shown. The scan still runs."
    : "Codex could not confirm Daybreak access, so some protected results may not be shown. The scan still runs.";
}

/** Starts a scan: picks the repository and type, and installs the plugin
 * only after the developer asks. */
export function ScanDialog({
  isOpen,
  onOpenChange,
  repo,
  onStarted,
}: {
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  /** The repository to scan, when chosen already. */
  repo?: string;
  onStarted?: (id: string) => void;
}) {
  const { snapshot: s, action } = useOrbit();
  const repos = s.settings.repositories.map((r) => r.name);
  const [chosen, setChosen] = useState(repo ?? repos[0] ?? "");
  const [mode, setMode] = useState<ScanMode>("standard");
  const [plugin, setPlugin] = useState<ScanPluginStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const running = (s.scans ?? []).find(scanActive);
  useEffect(() => {
    if (!isOpen) return;
    let current = true;
    void action<ScanPluginStatus>("get_scan_plugin", {}, { quiet: true }).then(
      (result) => {
        if (!current) return;
        if (result.ok) setPlugin(result.data);
        else setError(result.error);
      },
    );
    return () => {
      current = false;
    };
  }, [isOpen, action]);
  const close = (open: boolean) => {
    if (busy) return;
    onOpenChange(open);
    if (!open) {
      setPlugin(null);
      setError(null);
    }
  };
  const install = async () => {
    setBusy(true);
    setError(null);
    const result = await action<ScanPluginStatus>(
      "install_scan_plugin",
      {},
      { quiet: true },
    );
    setBusy(false);
    if (result.ok) setPlugin(result.data);
    else setError(result.error);
  };
  const start = async () => {
    setBusy(true);
    setError(null);
    const result = await action<string>(
      "start_scan",
      { repo: repo ?? chosen, mode },
      { quiet: true },
    );
    setBusy(false);
    if (!result.ok) return setError(result.error);
    onOpenChange(false);
    setPlugin(null);
    onStarted?.(result.data);
  };
  return (
    <Dialog
      isOpen={isOpen}
      onOpenChange={close}
      purpose={busy ? "required" : "form"}
      width={560}
    >
      <Layout
        header={
          <DialogHeader
            title="Run security scan"
            onOpenChange={busy ? undefined : close}
          />
        }
        content={
          <LayoutContent>
            <VStack gap={4}>
              <Text color="secondary">
                OpenAI’s Codex Security plugin reads the whole repository at the
                latest commit of its default branch, in a separate copy. Your
                checkout and pull request reviews are not affected. The findings
                appear on the Findings page.
              </Text>
              {error && <Banner status="error" title={error} />}
              {running && (
                <Banner
                  status="info"
                  title="A scan is already running"
                  description={`Orbit runs one scan at a time. Wait for the ${running.repo} scan to finish, or cancel it.`}
                />
              )}
              {repo ? (
                <Text weight="medium">{repo}</Text>
              ) : (
                <Selector
                  label="Repository"
                  value={chosen}
                  onChange={setChosen}
                  options={repos.map((name) => ({ value: name, label: name }))}
                />
              )}
              <RadioList
                label="Scan type"
                value={mode}
                onChange={(v) => setMode(v as ScanMode)}
              >
                <RadioListItem
                  value="standard"
                  label="Standard"
                  description="One complete pass over the repository. Expect it to take a while on a large codebase."
                />
                <RadioListItem
                  value="deep"
                  label="Deep"
                  description="Several independent standard passes, combined. Can take many hours and use gigabytes of disk space while it runs."
                />
              </RadioList>
              {!plugin && !error && (
                <Spinner label="Checking for the Codex Security plugin…" />
              )}
              {plugin && !plugin.installed && (
                <Banner
                  status="warning"
                  title="The Codex Security plugin is not installed"
                  description={
                    plugin.available
                      ? "Orbit can install it into your Codex setup. Nothing is installed unless you choose to."
                      : "Codex does not offer it. Update the Codex CLI, then try again."
                  }
                />
              )}
            </VStack>
          </LayoutContent>
        }
        footer={
          <LayoutFooter>
            <HStack hAlign="end" gap={2} wrap="wrap">
              <Button
                label="Cancel"
                variant="ghost"
                isDisabled={busy}
                onClick={() => close(false)}
              />
              {plugin && !plugin.installed ? (
                <Button
                  label="Install Codex Security"
                  variant="primary"
                  isLoading={busy}
                  isDisabled={busy || !plugin.available}
                  onClick={() => void install()}
                />
              ) : (
                <Button
                  label="Start scan"
                  variant="primary"
                  icon={<ShieldCheck />}
                  isLoading={busy}
                  isDisabled={
                    busy || !plugin?.installed || !!running || !(repo ?? chosen)
                  }
                  onClick={() => void start()}
                />
              )}
            </HStack>
          </LayoutFooter>
        }
      />
    </Dialog>
  );
}

/** A finished scan's report, as the plugin wrote it. */
export function ScanReportDialog({
  scan,
  onClose,
}: {
  scan: SecurityScan | null;
  onClose: () => void;
}) {
  const action = useOrbit((state) => state.action);
  const [report, setReport] = useState<{ id: string; text: string } | null>(
    null,
  );
  const [error, setError] = useState<{ id: string; text: string } | null>(null);
  useEffect(() => {
    if (!scan) return;
    let current = true;
    void action<string>(
      "get_scan_report",
      { scanId: scan.id },
      { quiet: true },
    ).then((result) => {
      if (!current) return;
      if (result.ok) setReport({ id: scan.id, text: result.data });
      else setError({ id: scan.id, text: result.error });
    });
    return () => {
      current = false;
    };
  }, [scan, action]);
  const text = report?.id === scan?.id ? report?.text : undefined;
  const failed = error?.id === scan?.id ? error?.text : undefined;
  return (
    <Dialog
      isOpen={!!scan}
      onOpenChange={(open) => !open && onClose()}
      purpose="info"
      width={820}
    >
      <Layout
        header={
          <DialogHeader
            title={scan ? `Security scan of ${scan.repo}` : "Security scan"}
            onOpenChange={(open) => !open && onClose()}
          />
        }
        content={
          <LayoutContent>
            {failed ? (
              <Banner status="error" title={failed} />
            ) : text === undefined ? (
              <Spinner label="Loading the report…" />
            ) : (
              <SafeMarkdown>{text}</SafeMarkdown>
            )}
          </LayoutContent>
        }
      />
    </Dialog>
  );
}

function ScanRow({
  scan,
  now,
  onReport,
  onFindings,
  onDelete,
}: {
  scan: SecurityScan;
  now: number;
  onReport: () => void;
  onFindings?: () => void;
  onDelete: () => void;
}) {
  const action = useOrbit((state) => state.action);
  const [cancelling, setCancelling] = useState(false);
  const active = scanActive(scan);
  const counts = severityCounts(scan);
  const warning = daybreakWarning(scan);
  const commit = scan.commit ? scan.commit.slice(0, 7) : null;
  return (
    <ListItem
      label={`${scan.repo} · ${scanModeLabels[scan.mode]} scan`}
      startContent={
        <Token
          label={scanStatusLabels[scan.status]}
          color={scanStatusColors[scan.status]}
          size="sm"
        />
      }
      endContent={
        <HStack gap={2} vAlign="center" wrap="wrap">
          {active ? (
            scan.status !== "importing" && (
              <Button
                label={
                  scan.status !== "cancelling"
                    ? "Cancel scan"
                    : scan.error
                      ? "Try stopping again"
                      : "Stopping"
                }
                size="sm"
                variant="ghost"
                icon={<Square />}
                isLoading={
                  cancelling || (scan.status === "cancelling" && !scan.error)
                }
                onClick={async () => {
                  setCancelling(true);
                  await action("cancel_scan", { scanId: scan.id });
                  setCancelling(false);
                }}
              />
            )
          ) : (
            <>
              {counts.map((n, p) =>
                n ? (
                  <Token
                    key={p}
                    label={`P${p} · ${n}`}
                    color={priorityColors[p]}
                    size="sm"
                  />
                ) : null,
              )}
              {scan.findings.length > 0 && onFindings && (
                <Button
                  label="Findings"
                  size="sm"
                  variant="ghost"
                  icon={<Bug />}
                  onClick={onFindings}
                />
              )}
              {scan.hasReport && (
                <Button
                  label="Report"
                  size="sm"
                  variant="ghost"
                  icon={<FileText />}
                  onClick={onReport}
                />
              )}
              <IconButton
                label={`Delete ${scan.repo} scan`}
                tooltip="Delete scan"
                size="sm"
                variant="ghost"
                icon={<Trash2 />}
                onClick={onDelete}
              />
            </>
          )}
        </HStack>
      }
      description={
        <VStack gap={2} paddingBlock={1}>
          <HStack gap={1} vAlign="center" wrap="wrap">
            <Text color="secondary">
              {[
                commit && `${scan.branch ?? "default branch"} @ ${commit}`,
                "started",
              ]
                .filter(Boolean)
                .join(" · ")}
            </Text>
            <Timestamp value={scan.createdAt} format="relative_short" />
            <Text color="secondary">
              {[
                `· ${active ? "running for " : ""}${scanDuration(scan, now)}`,
                scan.diskBytes != null && formatBytes(scan.diskBytes),
                !active &&
                  scan.status === "completed" &&
                  `${scan.findings.length} ${scan.findings.length === 1 ? "finding" : "findings"}`,
              ]
                .filter(Boolean)
                .join(" · ")}
            </Text>
          </HStack>
          {active && (
            <VStack gap={1}>
              <ProgressBar
                label={`${scan.repo} scan progress`}
                isLabelHidden
                isIndeterminate={!scan.progress?.total}
                value={scan.progress?.completed ?? 0}
                max={scan.progress?.total ?? 100}
              />
              <Text color="secondary">{progressLabel(scan)}</Text>
            </VStack>
          )}
          {scan.status === "cancelling" && scan.stopReason && (
            <Text color="secondary">{scan.stopReason}</Text>
          )}
          {warning && <Banner status="warning" title={warning} />}
          {scan.error && <Banner status="error" title={scan.error} />}
          {scan.warnings.map((w) => (
            <Text key={w} color="secondary">
              {w}
            </Text>
          ))}
        </VStack>
      }
    />
  );
}

/** Every security scan, with the running one's progress. */
export function ScansPage({ onFindings }: { onFindings?: () => void }) {
  const { snapshot: s, action } = useOrbit();
  const scans = s.scans ?? [];
  const now = useNow();
  const [starting, setStarting] = useState(false);
  const [report, setReport] = useState<SecurityScan | null>(null);
  const [deleting, setDeleting] = useState<SecurityScan | null>(null);
  const [busy, setBusy] = useState(false);
  return (
    <VStack gap={6} padding={6} maxWidth={1100} className="content-page">
      <HStack gap={4} hAlign="between" vAlign="start" wrap="wrap">
        <VStack gap={2}>
          <Heading level={2}>Security scans</Heading>
          <Text color="secondary">
            Whole-repository scans with OpenAI’s Codex Security plugin. Each
            runs on its own copy of the default branch, apart from pull request
            reviews. Their findings join the Findings page.
          </Text>
        </VStack>
        <Button
          label="Run security scan"
          variant="primary"
          icon={<ShieldCheck />}
          isDisabled={!s.settings.repositories.length}
          onClick={() => setStarting(true)}
        />
      </HStack>
      {scans.length === 0 ? (
        <EmptyState
          icon={<ShieldCheck />}
          title="No scans yet"
          description={
            s.settings.repositories.length
              ? "Run a scan to check a whole repository for security problems."
              : "Connect a repository first, then run a scan."
          }
        />
      ) : (
        <List hasDividers>
          {scans.map((scan) => (
            <ScanRow
              key={scan.id}
              scan={scan}
              now={now}
              onReport={() => setReport(scan)}
              onFindings={onFindings}
              onDelete={() => setDeleting(scan)}
            />
          ))}
        </List>
      )}
      <ScanDialog isOpen={starting} onOpenChange={setStarting} />
      <ScanReportDialog scan={report} onClose={() => setReport(null)} />
      <AlertDialog
        isOpen={!!deleting}
        onOpenChange={(open) => !open && setDeleting(null)}
        title="Delete scan"
        description={
          deleting
            ? `Delete the ${deleting.repo} scan from ${new Date(deleting.createdAt).toLocaleString()}? Its ${deleting.findings.length === 1 ? "finding leaves" : `${deleting.findings.length} findings leave`} the Findings page, and its report and files are removed.`
            : ""
        }
        actionLabel="Delete scan"
        isActionLoading={busy}
        onAction={async () => {
          if (!deleting || busy) return;
          setBusy(true);
          await action("delete_scan", { scanId: deleting.id });
          setBusy(false);
          setDeleting(null);
        }}
      />
    </VStack>
  );
}
