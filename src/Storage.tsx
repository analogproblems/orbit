import { useEffect, useRef, useState } from "react";
import { HardDrive, RefreshCw, Trash2 } from "lucide-react";
import { useOrbit, desktop } from "./store";
import { activeStatuses, type StorageUsage } from "./types";
import { Modal } from "./Modal";

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
    <section className="settings-section storage-section">
      <div className="section-heading">
        <h3>
          <HardDrive size={16} /> Storage
        </h3>
        {open ? (
          <button
            className="text-button"
            disabled={scanning || removing}
            onClick={() => void refresh()}
          >
            <RefreshCw size={14} />
            {scanning ? "Scanning…" : "Refresh storage"}
          </button>
        ) : (
          <button
            className="text-button"
            onClick={() => {
              setOpen(true);
              void refresh();
            }}
          >
            Inspect storage
          </button>
        )}
      </div>
      <p className="muted">
        Remove inactive review worktrees to free space. Reports and
        conversations stay available.
      </p>
      {removing && (
        <p role="status">
          Removing worktrees… You can keep using Orbit while cleanup finishes.
        </p>
      )}
      {cleanup.error && (
        <p role="alert" className="inline-error">
          {cleanup.error}
        </p>
      )}
      {open && (
        <>
          {!desktop && (
            <p className="action-notice">
              Demo storage sizes and cleanup are simulated.
            </p>
          )}
          {error && (
            <div role="alert" className="inline-error">
              {error}{" "}
              <button className="text-button" onClick={() => void refresh()}>
                Retry scan
              </button>
            </div>
          )}
          {usage && (
            <>
              <div className="storage-totals">
                <span>
                  Saved state <strong>{formatBytes(usage.stateBytes)}</strong>
                </span>
                <span>
                  Review worktrees{" "}
                  <strong>{formatBytes(usage.worktreeBytes)}</strong>
                </span>
                <span>
                  Unrecognized entries{" "}
                  <strong>{formatBytes(usage.unrecognizedBytes)}</strong>
                </span>
              </div>
              <small>
                Estimated file sizes; symlinks are not followed. Source clones
                and Codex’s own data are excluded. Unrecognized entries cannot
                be removed here.
              </small>
              {usage.errors.map((e, i) => (
                <p className="inline-error" role="alert" key={i}>
                  Incomplete scan: {e}
                </p>
              ))}
              <div className="storage-list">
                {usage.worktrees.map((w) => {
                  const r = snapshot.reviews.find((r) => r.id === w.reviewId);
                  return (
                    <label className="storage-row" key={w.reviewId}>
                      <input
                        type="checkbox"
                        checked={
                          selected.includes(w.reviewId) && allowed(w.reviewId)
                        }
                        disabled={
                          scanning ||
                          removing ||
                          !w.removable ||
                          !allowed(w.reviewId)
                        }
                        onChange={(e) =>
                          setSelected((ids) =>
                            e.target.checked
                              ? [...ids, w.reviewId]
                              : ids.filter((id) => id !== w.reviewId),
                          )
                        }
                      />
                      <span>
                        {describe(w.reviewId)}
                        <small>
                          {w.error
                            ? `Incomplete scan: ${w.error}`
                            : r?.cleanupPending
                              ? "Removing worktree…"
                              : !allowed(w.reviewId)
                                ? "Run is active or its workspace is unavailable"
                                : "Removing this worktree ends follow-ups for this run."}
                          {r?.cleanupError &&
                            ` Last cleanup failed: ${r.cleanupError}`}
                        </small>
                      </span>
                      <strong>{formatBytes(w.bytes)}</strong>
                    </label>
                  );
                })}
                {!usage.worktrees.length && <p>No saved review worktrees.</p>}
              </div>
              {usage.unrecognized.length > 0 && (
                <details className="unrecognized-storage">
                  <summary>Unrecognized entries (kept)</summary>
                  <ul>
                    {usage.unrecognized.map((entry) => (
                      <li key={entry.name}>
                        {entry.name} · {formatBytes(entry.bytes)}
                        {entry.error && (
                          <span role="alert">
                            {" "}
                            Incomplete scan: {entry.error}
                          </span>
                        )}
                      </li>
                    ))}
                  </ul>
                </details>
              )}
              <button
                className="button secondary"
                disabled={!chosen.length || scanning || removing}
                onClick={() => setConfirm(true)}
              >
                <Trash2 size={14} />
                Remove worktrees ({chosen.length})
              </button>
            </>
          )}
          {results.length > 0 && (
            <div aria-live="polite" className="cleanup-results">
              {results.map((r) => (
                <p key={r.reviewId} role={r.error ? "alert" : undefined}>
                  {describe(r.reviewId)}:{" "}
                  {r.error || "Worktree removed; report and chat retained."}
                  {r.warning && <span role="alert"> {r.warning}</span>}
                </p>
              ))}
            </div>
          )}
        </>
      )}
      {confirm && (
        <Modal
          title="Remove selected worktrees"
          onClose={() => setConfirm(false)}
          error={error}
        >
          <p>
            Remove {chosen.length} worktree{chosen.length === 1 ? "" : "s"},
            approximately {formatBytes(estimate)}? All files inside them,
            including dependencies and local changes, will be removed. Reports
            and chats stay readable, but these runs will no longer support
            follow-ups.
          </p>
          <ul>
            {chosen.map((w) => (
              <li key={w.reviewId}>{describe(w.reviewId)}</li>
            ))}
          </ul>
          <div className="modal-actions">
            <button
              className="button secondary"
              disabled={removing}
              onClick={() => setConfirm(false)}
            >
              Cancel
            </button>
            <button
              className="button primary"
              disabled={removing || !chosen.length}
              onClick={async () => {
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
            >
              {removing ? "Removing…" : "Confirm removal"}
            </button>
          </div>
        </Modal>
      )}
    </section>
  );
}
