// @vitest-environment jsdom
import "./test-setup";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { Findings } from "./Findings";
import { demoFindings, freshDemo } from "./demo";
import { useOrbit } from "./store";
import type { Review } from "./types";
afterEach(cleanup);
const review = (changes: Partial<Review>): Review => ({
  ...freshDemo().reviews[0],
  ...changes,
});
describe("structured findings", () => {
  it("lists each finding with its severity, location, and status", () => {
    const findings = demoFindings("r");
    findings[1] = { ...findings[1], path: null, endLine: 80 };
    render(<Findings review={review({ findings })} />);
    expect(
      screen.getByText("Verdict: changes requested · 2 findings"),
    ).toBeTruthy();
    expect(screen.getAllByText("P2")).toHaveLength(2);
    expect(screen.getByText("src/config/repositories.ts:48")).toBeTruthy();
    expect(screen.getByText("Outside the repository")).toBeTruthy();
    expect(screen.getAllByText("Open")).toHaveLength(2);
    expect(screen.getByText(/Not checked: End-to-end tests/)).toBeTruthy();
    // The category, the failure case, other places, and an unverified one.
    expect(screen.getByText("Data loss")).toBeTruthy();
    expect(screen.getByText(findings[0].failureScenario!)).toBeTruthy();
    expect(screen.getByText(findings[0].doneWhen!)).toBeTruthy();
    expect(screen.getByText("src/settings/save.ts:21–24")).toBeTruthy();
    expect(screen.getAllByText("Likely, not verified")).toHaveLength(1);
    // Short IDs as the PR's comments name them, and what blocks.
    expect(screen.getByText(`F1 · ${findings[0].title}`)).toBeTruthy();
    expect(screen.getByText(`F2 · ${findings[1].title}`)).toBeTruthy();
    expect(screen.getAllByText("Blocking")).toHaveLength(2);
    expect(screen.queryByText("A finding may be missing")).toBeNull();
  });
  it("says what blocks only when the review knows, and keeps older titles", () => {
    const findings = demoFindings("r").map((f) => ({ ...f, number: null }));
    render(
      <Findings review={review({ findings, blockingPriority: undefined })} />,
    );
    expect(screen.queryByText("Blocking")).toBeNull();
    expect(screen.getByText(findings[0].title)).toBeTruthy();
    cleanup();
    render(<Findings review={review({ findings, blockingPriority: 1 })} />);
    expect(screen.queryByText("Blocking")).toBeNull();
  });
  it("warns when the reviewer's report lists more findings than the list", () => {
    render(<Findings review={review({ reportFindings: 3 })} />);
    expect(screen.getByText("A finding may be missing")).toBeTruthy();
    expect(
      screen.getByText(
        "Codex’s report lists 3 findings, but Orbit’s list has 2, so some may have been left out. Check the report.",
      ),
    ).toBeTruthy();
  });
  it("shows how the setup and checks went in the dev container", () => {
    const checks = {
      image: "orbit-devcontainer:acme.orbit-abc",
      network: false,
      setup: {
        command: "npm ci",
        exitCode: 0,
        timedOut: false,
        seconds: 38,
        output: "",
      },
      check: {
        command: "npm test",
        exitCode: 1,
        timedOut: false,
        seconds: 64,
        output: "1 failed",
      },
    };
    render(<Findings review={review({ checks })} />);
    expect(
      screen.getByText("Setup in the dev container (npm ci): passed in 38s"),
    ).toBeTruthy();
    expect(
      screen.getByText(
        "Checks in the dev container (npm test): failed with exit code 1 after 64s",
      ),
    ).toBeTruthy();
    expect(screen.getByLabelText("Checks failed")).toBeTruthy();
    cleanup();
    render(
      <Findings
        review={review({
          checks: {
            ...checks,
            setup: { ...checks.setup, exitCode: null, timedOut: true },
            check: null,
          },
        })}
      />,
    );
    expect(
      screen.getByText(
        "Setup in the dev container (npm ci): stopped at its time limit after 38s",
      ),
    ).toBeTruthy();
    expect(
      screen.getByText("Checks didn't run, because setup failed."),
    ).toBeTruthy();
    cleanup();
    render(<Findings review={review({ checks: null })} />);
    expect(screen.queryByText(/in the dev container/)).toBeNull();
  });
  it("shows what older reviews could not run", () => {
    render(
      <Findings
        review={review({ notChecked: undefined, testsNotRun: ["cargo test"] })}
      />,
    );
    expect(screen.getByText("Not checked: cargo test")).toBeTruthy();
  });
  it("notes requests declined by the approval time limit", () => {
    const note =
      "Declined the reviewer's command `npm test` because no one approved it within 5 minutes.";
    render(<Findings review={review({ declinedApprovals: [note] })} />);
    expect(screen.getByText(note)).toBeTruthy();
    cleanup();
    // Also when the findings could not be extracted.
    render(
      <Findings
        review={review({ findings: undefined, declinedApprovals: [note] })}
      />,
    );
    expect(screen.getByText("Unstructured run")).toBeTruthy();
    expect(screen.getByText(note)).toBeTruthy();
  });
  it("labels unstructured runs and hides findings until a run finishes", () => {
    const { rerender } = render(
      <Findings review={review({ findings: null })} />,
    );
    expect(screen.getByText("Unstructured run")).toBeTruthy();
    rerender(
      <Findings review={review({ status: "reviewing", extracting: true })} />,
    );
    expect(screen.queryByText("Unstructured run")).toBeNull();
    expect(screen.queryByText(/Verdict/)).toBeNull();
  });
});
describe("Orbit's verdict", () => {
  it("counts blocking findings and notes when Codex disagrees", () => {
    const findings = demoFindings("r");
    const { rerender } = render(
      <Findings
        review={review({
          findings,
          verdict: "changes_requested",
          orbitVerdict: "clean",
          blocking: 0,
        })}
      />,
    );
    expect(
      screen.getByText("Verdict: clean · 2 findings, none blocking"),
    ).toBeTruthy();
    expect(screen.getByText(/Codex’s own verdict/)).toBeTruthy();
    rerender(
      <Findings
        review={review({
          findings,
          verdict: "changes_requested",
          orbitVerdict: "changes_requested",
          blocking: 1,
        })}
      />,
    );
    expect(
      screen.getByText("Verdict: changes requested · 1 blocking · 2 findings"),
    ).toBeTruthy();
    expect(screen.queryByText(/Codex’s own verdict/)).toBeNull();
  });
});
describe("since the last round", () => {
  const earlier = demoFindings("demo-completed");
  const reReview = review({
    id: "second",
    findings: [],
    verdict: "changes_requested",
    previousRound: {
      reviewId: "demo-completed",
      headSha: "abc0000",
      verdict: "changes_requested",
      findings: [...earlier, { ...earlier[0], id: "gone-0", title: "Old" }],
      decisions: [],
      claims: [
        {
          findingId: earlier[0].id,
          author: "alex",
          createdAt: "2026-01-02T00:00:00Z",
          kind: "fixed",
          commit: "abc1234def",
          commitCheck: "in_change",
          reason: null,
        },
        {
          findingId: earlier[1].id,
          author: "alex",
          createdAt: "2026-01-02T00:00:00Z",
          kind: "fixed",
          commit: "0000000",
          commitCheck: "not_found",
          reason: null,
        },
      ],
      commentsIncluded: 1,
      commentsOmitted: 2,
    },
    priorFindings: [
      { id: earlier[0].id, status: "fixed", note: "Merged now." },
      { id: earlier[1].id, status: "declined_disputed", note: "Still racy." },
    ],
  });
  it("summarizes what happened to each earlier finding", () => {
    useOrbit.setState({ snapshot: freshDemo() });
    render(<Findings review={reReview} onOpenReview={() => {}} />);
    expect(screen.getByText("Since the last round")).toBeTruthy();
    expect(
      screen.getByText("1 fixed, 1 declined (disputed), 1 not reported"),
    ).toBeTruthy();
    expect(screen.getByText("Merged now.")).toBeTruthy();
    expect(screen.getByText("Still racy.")).toBeTruthy();
    expect(screen.getByText("Not reported")).toBeTruthy();
    // What the author said in a reply Orbit recognised, and where its
    // commit is when that matters.
    expect(screen.getByText("@alex: fixed in abc1234.")).toBeTruthy();
    expect(
      screen.getByText(
        "@alex: fixed in 0000000. Orbit couldn’t find that commit.",
      ),
    ).toBeTruthy();
    // Only findings whose review still exists link back to it.
    expect(screen.getAllByRole("button", { name: "Open review" })).toHaveLength(
      2,
    );
  });
  it("opens the review that first reported a finding", () => {
    useOrbit.setState({ snapshot: freshDemo() });
    const opened = vi.fn();
    render(<Findings review={reReview} onOpenReview={opened} />);
    fireEvent.click(screen.getAllByRole("button", { name: "Open review" })[0]);
    expect(opened).toHaveBeenCalledWith("demo-completed");
  });
  it("is absent on a first review", () => {
    render(<Findings review={review({})} />);
    expect(screen.queryByText("Since the last round")).toBeNull();
  });
});
