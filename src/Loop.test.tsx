// @vitest-environment jsdom
import "./test-setup";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { LoopPanel, ReadinessPanel } from "./Loop";
import { freshDemo } from "./demo";
import { useOrbit } from "./store";
afterEach(cleanup);
describe("review loop panel", () => {
  it("explains why the loop needs a person and offers to continue it", () => {
    const snapshot = freshDemo();
    const action = vi.fn(async () => ({ ok: true, data: undefined }) as never);
    useOrbit.setState({ snapshot, action });
    const prId = snapshot.prs[1].id;
    render(<LoopPanel prId={prId} />);
    expect(screen.getByText("Loop: Needs you")).toBeTruthy();
    expect(screen.getByText("Round 4 of 4")).toBeTruthy();
    expect(screen.getByText("Round limit reached.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Continue loop" }));
    expect(action).toHaveBeenCalledWith("continue_loop", { prId, rounds: 2 });
  });
  it("offers Stop while the loop runs and shows nothing without a loop", () => {
    const snapshot = freshDemo();
    snapshot.loopStates![0] = {
      ...snapshot.loopStates![0],
      status: "waiting_for_author",
      reason: null,
      rounds: 1,
    };
    const action = vi.fn(async () => ({ ok: true, data: undefined }) as never);
    useOrbit.setState({ snapshot, action });
    const { container } = render(<LoopPanel prId={snapshot.prs[0].id} />);
    expect(container.textContent).toBe("");
    render(<LoopPanel prId={snapshot.prs[1].id} />);
    fireEvent.click(screen.getByRole("button", { name: "Stop loop" }));
    expect(action).toHaveBeenCalledWith("stop_loop", {
      prId: snapshot.prs[1].id,
    });
  });
  it("says when GitHub refused the status check", () => {
    const snapshot = freshDemo();
    snapshot.settings.autoPost = true;
    const prId = snapshot.prs[1].id;
    snapshot.commitChecks = [
      {
        prId,
        repo: snapshot.prs[1].repo,
        sha: snapshot.prs[1].headSha,
        refused: {
          state: "failure",
          description: "Round 4 · 1 blocking finding",
          error: "Resource not accessible by integration",
        },
      },
    ];
    useOrbit.setState({ snapshot });
    render(<LoopPanel prId={prId} />);
    expect(
      screen.getByText(
        "GitHub did not accept the status check: Resource not accessible by integration",
      ),
    ).toBeTruthy();
  });
});
describe("merging panel", () => {
  it("says what is in the way, first things first", () => {
    const snapshot = freshDemo();
    useOrbit.setState({ snapshot });
    render(<ReadinessPanel prId={snapshot.prs[1].id} />);
    expect(screen.getByText("Not clear yet")).toBeTruthy();
    // Orbit hasn't told its agent anything yet.
    expect(
      screen.queryByRole("button", { name: "Open Orbit's comment" }),
    ).toBeNull();
  });
  it("opens Orbit's comment only once it gives the answer on GitHub", () => {
    const snapshot = freshDemo();
    const told = snapshot.ops!.find((t) => t.ready)!;
    const prId = snapshot.prs[1].id;
    // A comment only about CI, and one not on GitHub yet.
    snapshot.ops = [
      { ...told, prId, ready: undefined },
      { ...told, prId, url: "" },
    ];
    useOrbit.setState({ snapshot });
    render(<ReadinessPanel prId={prId} />);
    expect(screen.getByText("Not clear yet")).toBeTruthy();
    expect(
      screen.queryByRole("button", { name: "Open Orbit's comment" }),
    ).toBeNull();
    expect(
      screen.getByText("Orbit's review loop needs you: Round limit reached."),
    ).toBeTruthy();
    expect(
      screen.getByText("Orbit hasn't read the checks on abc1111 yet."),
    ).toBeTruthy();
  });
  it("says where it is clear to merge, and nothing where Orbit has no answer", () => {
    const snapshot = freshDemo();
    snapshot.readiness![1] = {
      ...snapshot.readiness![1],
      baseSha: "7d41e09" + "0".repeat(33),
      clear: true,
      waits: [],
    };
    useOrbit.setState({ snapshot });
    const { container } = render(<ReadinessPanel prId={snapshot.prs[0].id} />);
    expect(container.textContent).toBe("");
    render(<ReadinessPanel prId={snapshot.prs[4].id} />);
    expect(screen.getByText("Clear to merge")).toBeTruthy();
    expect(
      screen.getByText("Clear to merge at abc4444 against main at 7d41e09."),
    ).toBeTruthy();
  });
  it("says whom Orbit nudged about an answer that has stood a while", () => {
    const snapshot = freshDemo();
    const pr = snapshot.prs[4];
    const reminded =
      "Orbit reminded the author once on its comment: this has waited on them for more than 2 hours.";
    const inNeedsYou =
      "This has waited on a person for more than a day, so it is in Needs you.";
    const shows = (s: typeof snapshot) => {
      cleanup();
      useOrbit.setState({ snapshot: s });
      render(<ReadinessPanel prId={pr.id} />);
      return [reminded, inNeedsYou].filter((t) => screen.queryByText(t));
    };
    // The demo's agent was reminded.
    expect(shows(snapshot)).toEqual([reminded]);
    const told = (s: typeof snapshot) =>
      s.ops!.find((t) => t.prId === pr.id)!.ready!;
    // Not before Orbit reminded it, nor once it is someone else's move,
    // nor where a person was nudged instead.
    const quiet = structuredClone(snapshot);
    told(quiet).nudged = null;
    expect(shows(quiet)).toEqual([]);
    const theirs = structuredClone(snapshot);
    theirs.readiness![1].waits = [
      {
        kind: "reviews",
        turn: "person",
        text: "main needs 1 approval of abc4444 and it has none: bob hasn't reviewed abc4444 yet.",
      },
    ];
    expect(shows(theirs)).toEqual([]);
    const person = structuredClone(snapshot);
    told(person).turn = "person";
    expect(shows(person)).toEqual([]);
    // A person's move a day on is in Needs you; another's item isn't.
    const item = {
      kind: "stuck" as const,
      reviewId: null,
      approvalId: null,
      repo: null,
      reason: `It has waited on a person for more than a day. ${theirs.readiness![1].waits[0].text}`,
    };
    theirs.attention = [
      { ...item, key: "stuck:other", prId: snapshot.prs[1].id },
      { ...item, kind: "loop", key: `loop:${pr.id}`, prId: pr.id },
    ];
    expect(shows(theirs)).toEqual([]);
    theirs.attention.push({ ...item, key: `stuck:${pr.id}`, prId: pr.id });
    expect(shows(theirs)).toEqual([inNeedsYou]);
  });
  it("lists who was asked to review it and what the branch's rules need", () => {
    const snapshot = freshDemo();
    const head = "abc4444" + "0".repeat(33);
    const old = "9f8e7d6" + "0".repeat(33);
    snapshot.readiness![1] = {
      ...snapshot.readiness![1],
      headSha: head,
      waits: [
        {
          kind: "reviews",
          turn: "person",
          text: "main needs 2 approvals of abc4444 and it has 1: bob hasn't reviewed abc4444 yet.",
        },
      ],
      reviews: {
        reviewers: [
          { name: "bob", team: false, stand: "asked", commit: "" },
          { name: "acme/devs", team: true, stand: "asked", commit: "" },
          {
            name: "alice",
            team: false,
            stand: "approved",
            commit: head,
            mayApprove: true,
          },
          { name: "carol", team: false, stand: "approved_older", commit: old },
          {
            name: "dan",
            team: false,
            stand: "changes_requested",
            commit: old,
            mayApprove: true,
          },
          {
            name: "erin",
            team: false,
            stand: "changes_requested",
            commit: head,
            mayApprove: true,
          },
          {
            name: "mallory",
            team: false,
            stand: "approved",
            commit: head,
            mayApprove: false,
          },
          {
            name: "trent",
            team: false,
            stand: "approved",
            commit: head,
            mayApprove: null,
          },
          {
            name: "oscar",
            team: false,
            stand: "changes_requested",
            commit: old,
            mayApprove: false,
          },
          // Orbit didn't ask.
          { name: "victor", team: false, stand: "approved", commit: head },
        ],
        rule: { approvals: 2, codeOwners: true },
      },
    };
    useOrbit.setState({ snapshot });
    render(<ReadinessPanel prId={snapshot.prs[4].id} />);
    expect(
      screen.getByText(
        "main needs 2 approvals of abc4444 and it has 1: bob hasn't reviewed abc4444 yet.",
      ),
    ).toBeTruthy();
    const list = screen.getByRole("list", { name: "Reviewers" });
    expect(list.textContent).toBe(
      [
        "bobHasn’t reviewed abc4444 yet.",
        "acme/devs teamHasn’t reviewed abc4444 yet.",
        "aliceApproved abc4444.",
        "carolApproved 9f8e7d6, but not abc4444.",
        "danAsked for changes on 9f8e7d6, and hasn’t reviewed abc4444 since.",
        "erinAsked for changes on abc4444.",
        "malloryApproved abc4444, but without write access that doesn’t count.",
        "trentApproved abc4444, but Orbit can’t tell whether they have write access.",
        "oscarAsked for changes on 9f8e7d6, but without write access that doesn’t hold it back.",
        "victorApproved abc4444, but Orbit can’t tell whether they have write access.",
      ].join(""),
    );
    expect(screen.getAllByLabelText("Approval doesn’t count")).toHaveLength(3);
    expect(screen.getAllByLabelText("Request doesn’t count")).toHaveLength(1);
    expect(screen.getAllByLabelText("Approved")).toHaveLength(1);
    expect(screen.getAllByLabelText("Asked for changes")).toHaveLength(2);
    for (const label of [
      "Asked",
      "Approved",
      "Approved an older commit",
      "Asked for changes",
    ])
      expect(screen.getAllByLabelText(label).length).toBeGreaterThan(0);
    expect(
      screen.getByText(
        "main’s rules need 2 approvals, and a code owner’s approval, which GitHub checks. Only approvals of abc4444, its latest commit, count.",
      ),
    ).toBeTruthy();
    // Other rules, and rules Orbit can't see.
    const rules: [{ approvals: number; codeOwners: boolean } | null, string][] =
      [
        [
          { approvals: 1, codeOwners: false },
          "main’s rules need 1 approval. Only approvals of abc4444, its latest commit, count.",
        ],
        [
          { approvals: 0, codeOwners: true },
          "main’s rules need a code owner’s approval, which GitHub checks.",
        ],
        [
          { approvals: 0, codeOwners: false },
          "main’s rules need no approvals.",
        ],
        [null, "Orbit can’t see main’s rules."],
      ];
    for (const [rule, text] of rules) {
      cleanup();
      snapshot.readiness![1].reviews = { reviewers: [], rule };
      useOrbit.setState({ snapshot: structuredClone(snapshot) });
      render(<ReadinessPanel prId={snapshot.prs[4].id} />);
      expect(screen.getByText(text)).toBeTruthy();
      expect(screen.getByText("Nobody is asked to review it.")).toBeTruthy();
    }
    cleanup();
    snapshot.readiness![1].reviews = {
      reviewers: [],
      rule: null,
      unseen: "Orbit's GitHub sign-in can't see main's rules.",
    };
    useOrbit.setState({ snapshot: structuredClone(snapshot) });
    render(<ReadinessPanel prId={snapshot.prs[4].id} />);
    expect(
      screen.getByText("Orbit's GitHub sign-in can't see main's rules."),
    ).toBeTruthy();
    // Reviews Orbit couldn't read.
    for (const [reviews, text] of [
      [
        { reviewers: [], tooMany: true },
        "It has more reviews than Orbit reads, so Orbit can’t list them.",
      ],
      [
        { reviewers: [], unread: "Bad Gateway (HTTP 502)" },
        "Orbit couldn’t read its reviews: Bad Gateway (HTTP 502).",
      ],
    ] as const) {
      cleanup();
      snapshot.readiness![1].reviews = { ...reviews, reviewers: [] };
      useOrbit.setState({ snapshot: structuredClone(snapshot) });
      render(<ReadinessPanel prId={snapshot.prs[4].id} />);
      expect(screen.getByText(text)).toBeTruthy();
      expect(screen.queryByText("Nobody is asked to review it.")).toBeNull();
    }
  });
  it("says nothing of reviews before Orbit has read them", () => {
    const snapshot = freshDemo();
    useOrbit.setState({ snapshot });
    render(<ReadinessPanel prId={snapshot.prs[4].id} />);
    expect(screen.queryByText("Reviews")).toBeNull();
  });
  it("opens the comment where Orbit tells the agent", () => {
    const snapshot = freshDemo();
    useOrbit.setState({ snapshot });
    const open = vi.spyOn(window, "open").mockImplementation(() => null);
    render(<ReadinessPanel prId={snapshot.prs[4].id} />);
    fireEvent.click(
      screen.getByRole("button", { name: "Open Orbit's comment" }),
    );
    expect(open).toHaveBeenCalledWith(
      `${snapshot.prs[4].url}#issuecomment-902`,
      "_blank",
      "noopener,noreferrer",
    );
    open.mockRestore();
  });
});
