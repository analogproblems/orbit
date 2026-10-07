// @vitest-environment jsdom
import "./test-setup";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { FindingsPage, filterFindings, noFilters } from "./FindingsPage";
import { collectFindings } from "./findingIndex";
import { freshDemo } from "./demo";
import { useOrbit } from "./store";
import type { AttentionHandlers } from "./types";
afterEach(cleanup);

function Page({ onChoose }: Pick<AttentionHandlers, "onChoose">) {
  const [selected, setSelected] = useState<string | null>(null);
  return (
    <FindingsPage
      selected={selected}
      onSelect={setSelected}
      onChoose={onChoose}
    />
  );
}

describe("findings page", () => {
  it("lists findings with an overview, and opens one with its history", () => {
    const snapshot = freshDemo();
    useOrbit.setState({ snapshot });
    const onChoose = vi.fn();
    render(<Page onChoose={onChoose} />);
    const rows = collectFindings(snapshot);
    expect(rows.length).toBeGreaterThan(0);
    const label = rows.length === 1 ? "finding" : "findings";
    expect(screen.getByText(`${rows.length} ${label}`)).toBeTruthy();
    expect(screen.getByRole("columnheader", { name: "Severity" })).toBeTruthy();
    // A finding carried through later rounds shows each decision.
    const followed = rows.find((r) => r.history.length > 1)!;
    const same = rows.filter((r) => r.finding.title === followed.finding.title);
    fireEvent.click(
      screen.getAllByRole("button", {
        name: (name) => name.includes(followed.finding.title),
      })[same.indexOf(followed)],
    );
    expect(
      screen.getByRole("heading", { name: followed.finding.title }),
    ).toBeTruthy();
    expect(screen.getByText("History")).toBeTruthy();
    expect(screen.getByText("Round 1: found")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Open review chat" }));
    expect(onChoose).toHaveBeenCalledWith(followed.pr, followed.review);
    fireEvent.click(screen.getByRole("button", { name: "All findings" }));
    expect(screen.getByText(`${rows.length} ${label}`)).toBeTruthy();
  });
  it("filters by search, severity, status and PR state", () => {
    const rows = collectFindings(freshDemo());
    const p2 = filterFindings(rows, { ...noFilters, severity: "2" });
    expect(p2.length).toBeGreaterThan(0);
    expect(p2.every((r) => r.finding.priority === 2)).toBe(true);
    const fixed = filterFindings(rows, { ...noFilters, status: "fixed" });
    expect(fixed.every((r) => r.status === "fixed")).toBe(true);
    const title = rows[0].finding.title.toUpperCase();
    expect(
      filterFindings(rows, { ...noFilters, search: title }).length,
    ).toBeGreaterThan(0);
    expect(filterFindings(rows, { ...noFilters, search: "zzzz" })).toEqual([]);
    expect(filterFindings(rows, { ...noFilters, pr: "closed" })).toEqual(
      rows.filter((r) => r.source === "review" && !r.prOpen),
    );
    const scanned = filterFindings(rows, { ...noFilters, source: "scan" });
    expect(scanned.length).toBeGreaterThan(0);
    expect(scanned.every((r) => r.scan && !r.pr)).toBe(true);
    expect(filterFindings(rows, { ...noFilters, pr: "open" })).toEqual(
      rows.filter((r) => r.prOpen),
    );
  });
  it("explains an empty page", () => {
    const snapshot = freshDemo();
    snapshot.reviews = [];
    snapshot.scans = [];
    useOrbit.setState({ snapshot });
    render(<Page onChoose={vi.fn()} />);
    expect(screen.getByText("No findings yet")).toBeTruthy();
  });
  it("shows a security scan's finding with its report", async () => {
    const snapshot = freshDemo();
    snapshot.reviews = [];
    useOrbit.setState({ snapshot });
    render(<Page onChoose={vi.fn()} />);
    const scan = snapshot.scans![0];
    const finding = scan.findings[0];
    expect(screen.getAllByText(`Security scan · ${scan.repo} ·`).length).toBe(
      scan.findings.length,
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: (name) => name.includes(finding.title),
      }),
    );
    expect(
      screen.getByText(
        `Security scan of ${scan.repo} at ${scan.commit!.slice(0, 7)}`,
      ),
    ).toBeTruthy();
    expect(screen.getByText("Scanned commit")).toBeTruthy();
    expect(screen.queryByText("History")).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Open review chat" }),
    ).toBeNull();
    const action = useOrbit.getState().action;
    const read = vi.fn(async () => ({ ok: true, data: "# Scan report" }));
    useOrbit.setState({ action: read as unknown as typeof action });
    try {
      fireEvent.click(screen.getByRole("button", { name: "Read scan report" }));
      expect(
        await screen.findByRole("heading", { name: "Scan report" }),
      ).toBeTruthy();
      expect(read).toHaveBeenCalledWith(
        "get_scan_report",
        { scanId: scan.id },
        { quiet: true },
      );
    } finally {
      useOrbit.setState({ action });
    }
  });
});
