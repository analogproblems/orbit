// @vitest-environment jsdom
import "./test-setup";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { PublishPreview, readable } from "./PublishPreview";
import { demoFindings, freshDemo } from "./demo";
import { useOrbit } from "./store";
import type { Review } from "./types";
afterEach(cleanup);
const review = (changes: Partial<Review>): Review => ({
  ...freshDemo().reviews[0],
  ...changes,
});
const respond = (result: unknown) => {
  const action = vi.fn().mockResolvedValue(result);
  useOrbit.setState({ action } as never);
  return action;
};
describe("publish preview", () => {
  it("shows where each finding goes and the review text", async () => {
    const findings = demoFindings("r");
    const action = respond({
      ok: true,
      data: {
        placements: [
          { findingId: findings[0].id, inline: true, reason: null },
          {
            findingId: findings[1].id,
            inline: false,
            reason: "lines not in the diff",
          },
        ],
        body: '<!-- orbit-review:r -->\n## Orbit · Codex review\n\nOne defect.\n\n<details>\n<summary>Codex’s own report</summary>\n\nThe whole report.\n\n</details>\n\n<!-- orbit-data {"version":1} -->\n---\nModel',
      },
    });
    render(<PublishPreview review={review({ id: "r", findings })} />);
    expect(
      await screen.findByText(
        "1 of 2 findings go on the lines they are about. The rest goes in the review text.",
      ),
    ).toBeTruthy();
    expect(screen.getByText("On its lines")).toBeTruthy();
    // An inline finding's text is not in the review text, so the row shows it.
    expect(screen.getByText(findings[0].body)).toBeTruthy();
    // So does the rest of what will be posted with it.
    expect(screen.getByText(findings[0].failureScenario!)).toBeTruthy();
    expect(screen.getByText(findings[0].doneWhen!)).toBeTruthy();
    expect(screen.getByText("src/settings/save.ts:21–24")).toBeTruthy();
    expect(screen.getByText("Likely, not verified")).toBeTruthy();
    expect(
      screen.getByText("In the review text: lines not in the diff"),
    ).toBeTruthy();
    expect(screen.getByText("One defect.")).toBeTruthy();
    // Short IDs and what blocks, as the comments will say.
    expect(screen.getByText(`F1 · ${findings[0].title}`)).toBeTruthy();
    expect(screen.getAllByText("Blocking")).toHaveLength(2);
    // The review text reads as GitHub shows it, with the hidden data noted.
    expect(screen.getByText("The whole report.")).toBeTruthy();
    expect(screen.queryByText(/orbit-data/)).toBeNull();
    expect(
      screen.getByText(
        "The review also carries a hidden copy of every finding for agents to read.",
      ),
    ).toBeTruthy();
    expect(action).toHaveBeenCalledWith(
      "publish_plan",
      { reviewId: "r" },
      { quiet: true },
    );
  });
  it("shows the review text as GitHub does", () => {
    expect(
      readable(
        '<!-- orbit-review:r -->\n<!-- orbit-verdict: clean blocking=0 -->\n## Orbit\n\n<details>\n<summary>Claude’s own report</summary>\n\nReport.\n\n</details>\n\n<!-- orbit-data {"a":"\\u003e"} -->\n---\nModel',
      ),
    ).toBe(
      "## Orbit\n\n**Claude’s own report** (folded on GitHub)\n\nReport.\n\n\n---\nModel",
    );
  });
  it("says when the diff could not be checked", async () => {
    respond({ ok: false, error: "GitHub timed out." });
    render(<PublishPreview review={review({ findings: demoFindings("r") })} />);
    expect(await screen.findByText("Couldn’t check the diff")).toBeTruthy();
    expect(
      screen.getByText(/Orbit checks it again when publishing/),
    ).toBeTruthy();
  });
  it("shows the report for an unstructured run without checking the diff", () => {
    const action = respond({ ok: true, data: null });
    render(
      <PublishPreview
        review={review({ findings: null, report: "Whole report." })}
      />,
    );
    expect(screen.getByText("Whole report.")).toBeTruthy();
    expect(action).not.toHaveBeenCalled();
  });
});
