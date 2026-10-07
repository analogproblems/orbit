// @vitest-environment jsdom
import "./test-setup";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import {
  ScanDialog,
  ScansPage,
  progressLabel,
  scanDuration,
  severityCounts,
} from "./Scans";
import { demoScan, freshDemo } from "./demo";
import { useOrbit } from "./store";
import type { ActionResult, SecurityScan } from "./types";
afterEach(cleanup);

/** Replaces the store's action for one test, answering each command. */
function mockActions(answers: Record<string, unknown>) {
  const original = useOrbit.getState().action;
  const calls: [string, Record<string, unknown> | undefined][] = [];
  const action = vi.fn(
    async (command: string, args?: Record<string, unknown>) => {
      calls.push([command, args]);
      const answer = answers[command];
      if (answer instanceof Error)
        return { ok: false, error: answer.message } as ActionResult<unknown>;
      return { ok: true, data: answer } as ActionResult<unknown>;
    },
  );
  useOrbit.setState({ action: action as unknown as typeof original });
  return { calls, restore: () => useOrbit.setState({ action: original }) };
}
function running(changes: Partial<SecurityScan> = {}): SecurityScan {
  return {
    ...demoScan("acme/orbit", 90),
    id: "scan-running",
    status: "running",
    finishedAt: null,
    findings: [],
    hasReport: false,
    diskBytes: null,
    ...changes,
  };
}

describe("security scans", () => {
  it("describes progress, duration and severity", () => {
    const scan = running({
      progress: {
        phase: "discovery",
        completed: 3,
        total: 8,
        unit: "reviews",
        findings: 1,
        updatedAt: "",
      },
    });
    expect(progressLabel(scan)).toBe(
      "Looking for vulnerabilities · 3 of 8 reviews · 1 finding so far",
    );
    expect(progressLabel({ ...scan, progress: null })).toBe(
      "Starting the Codex Security scan",
    );
    expect(progressLabel({ ...scan, status: "preparing" })).toBe(
      "Copying the repository at its latest commit",
    );
    const start = Date.parse(scan.createdAt);
    expect(scanDuration(scan, start + 30_000)).toBe("under a minute");
    expect(scanDuration(scan, start + 45 * 60_000)).toBe("45 min");
    expect(scanDuration(scan, start + 125 * 60_000)).toBe("2 h 5 min");
    expect(severityCounts(demoScan("acme/api", 60))).toEqual([0, 1, 1, 1]);
  });
  it("lists scans, shows the running one's progress, and cancels it", async () => {
    const snapshot = freshDemo();
    snapshot.scans = [
      running({
        mode: "deep",
        daybreak: "not_granted",
        progress: { phase: "validation", updatedAt: "" },
      }),
      ...snapshot.scans!,
    ];
    useOrbit.setState({ snapshot });
    const { calls, restore } = mockActions({ cancel_scan: null });
    try {
      render(<ScansPage />);
      expect(screen.getByText("acme/orbit · Deep scan")).toBeTruthy();
      expect(screen.getByText("Checking candidate findings")).toBeTruthy();
      expect(screen.getByText(/does not have Daybreak access/)).toBeTruthy();
      expect(screen.getByText("acme/api · Standard scan")).toBeTruthy();
      expect(screen.getByText("P1 · 1")).toBeTruthy();
      fireEvent.click(screen.getByRole("button", { name: "Cancel scan" }));
      await waitFor(() =>
        expect(calls).toContainEqual([
          "cancel_scan",
          { scanId: "scan-running" },
        ]),
      );
    } finally {
      restore();
    }
  });
  it("asks before deleting a scan", async () => {
    const snapshot = freshDemo();
    useOrbit.setState({ snapshot });
    const { calls, restore } = mockActions({ delete_scan: null });
    try {
      render(<ScansPage />);
      fireEvent.click(
        screen.getByRole("button", { name: "Delete acme/api scan" }),
      );
      expect(
        screen.getByText(/3 findings leave the Findings page/),
      ).toBeTruthy();
      expect(calls.some(([c]) => c === "delete_scan")).toBe(false);
      fireEvent.click(screen.getByRole("button", { name: "Delete scan" }));
      await waitFor(() =>
        expect(calls).toContainEqual([
          "delete_scan",
          { scanId: snapshot.scans![0].id },
        ]),
      );
    } finally {
      restore();
    }
  });
  it("installs the plugin only when asked, then starts a scan", async () => {
    const snapshot = freshDemo();
    snapshot.scans = [];
    useOrbit.setState({ snapshot });
    const { calls, restore } = mockActions({
      get_scan_plugin: { available: true, installed: false },
      install_scan_plugin: { available: true, installed: true },
      start_scan: "scan-new",
    });
    const onStarted = vi.fn();
    try {
      render(
        <ScanDialog
          isOpen
          onOpenChange={() => {}}
          repo="acme/api"
          onStarted={onStarted}
        />,
      );
      expect(
        await screen.findByText("The Codex Security plugin is not installed"),
      ).toBeTruthy();
      expect(calls.map(([c]) => c)).toEqual(["get_scan_plugin"]);
      fireEvent.click(
        screen.getByRole("button", { name: "Install Codex Security" }),
      );
      const start = await screen.findByRole("button", { name: "Start scan" });
      fireEvent.click(screen.getByRole("radio", { name: /Deep/ }));
      fireEvent.click(start);
      await waitFor(() => expect(onStarted).toHaveBeenCalledWith("scan-new"));
      expect(calls.map(([c, a]) => [c, a])).toEqual([
        ["get_scan_plugin", {}],
        ["install_scan_plugin", {}],
        ["start_scan", { repo: "acme/api", mode: "deep" }],
      ]);
    } finally {
      restore();
    }
  });
  it("does not start a second scan while one runs", async () => {
    const snapshot = freshDemo();
    snapshot.scans = [running()];
    useOrbit.setState({ snapshot });
    const { restore } = mockActions({
      get_scan_plugin: { available: true, installed: true },
    });
    try {
      render(<ScanDialog isOpen onOpenChange={() => {}} />);
      expect(screen.getByText("A scan is already running")).toBeTruthy();
      const start = await screen.findByRole("button", { name: "Start scan" });
      expect(
        start.hasAttribute("disabled") ||
          start.getAttribute("aria-disabled") === "true",
      ).toBe(true);
    } finally {
      restore();
    }
  });
});
