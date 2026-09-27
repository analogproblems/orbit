// @vitest-environment jsdom
import "./test-setup";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { PublishPreview } from "./PublishPreview";
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
        body: "## Orbit · Codex review\n\nOne defect.",
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
    expect(
      screen.getByText("In the review text: lines not in the diff"),
    ).toBeTruthy();
    expect(screen.getByText("One defect.")).toBeTruthy();
    expect(action).toHaveBeenCalledWith(
      "publish_plan",
      { reviewId: "r" },
      { quiet: true },
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
