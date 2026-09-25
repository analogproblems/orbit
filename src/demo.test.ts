import { describe, expect, it } from "vitest";
import { demoCodexConfig, freshDemo, resolveDemoModelSelection } from "./demo";

describe("demo model migration", () => {
  it("uses simulated Codex configuration before the catalog default and names both sources", () => {
    const models = freshDemo().models;
    expect(models[0].defaultReasoningEffort).toBe("medium");
    const result = resolveDemoModelSelection({ model: "", effort: "" }, models);
    expect(result.selection).toEqual({
      model: demoCodexConfig.model,
      effort: "high",
    });
    expect(result.notice).toContain("Model: your Codex configuration.");
    expect(result.notice).toContain("Effort: your Codex configuration.");
  });

  it("resolves only a missing effort using config even when the configured model differs", () => {
    const result = resolveDemoModelSelection(
      { model: demoCodexConfig.model, effort: "" },
      freshDemo().models,
      { model: "another-model", model_reasoning_effort: "low" },
    );
    expect(result.selection).toEqual({
      model: demoCodexConfig.model,
      effort: "low",
    });
    expect(result.notice).toContain("Model: your saved Orbit selection.");
    expect(result.notice).toContain("Effort: your Codex configuration.");
  });

  it("explains unsupported, absent, and unreadable configured-effort fallbacks", () => {
    for (const [config, reason] of [
      [
        { model_reasoning_effort: "xhigh" },
        "your configured effort 'xhigh' isn't supported by this model",
      ],
      [{ model_reasoning_effort: null }, "no configured effort"],
      [new Error("offline"), "Codex configuration could not be read"],
    ] as const) {
      const result = resolveDemoModelSelection(
        { model: demoCodexConfig.model, effort: "" },
        freshDemo().models,
        config,
      );
      expect(result.selection.effort).toBe("medium");
      expect(result.notice).toContain(`Effort: model default (${reason}).`);
    }
    const defaults = resolveDemoModelSelection(
      { model: "", effort: "" },
      freshDemo().models,
      {},
    );
    expect(defaults.notice).toContain(
      "Model: catalog default (no configured model).",
    );
  });

  it("keeps explicit efforts and blocks invalid selections or unresolved defaults", () => {
    const models = freshDemo().models;
    const partial = resolveDemoModelSelection(
      { model: "", effort: "low" },
      models,
    );
    expect(partial.selection.effort).toBe("low");
    expect(partial.notice).toContain("Effort: your saved Orbit selection.");
    const explicit = { model: demoCodexConfig.model, effort: "low" };
    expect(resolveDemoModelSelection(explicit, models)).toEqual({
      selection: explicit,
      notice: undefined,
    });
    expect(() =>
      resolveDemoModelSelection({ ...explicit, effort: "unsupported" }, models),
    ).toThrow("does not support");
    expect(() =>
      resolveDemoModelSelection(
        { model: "", effort: "" },
        models,
        new Error("offline"),
      ),
    ).toThrow("Could not read effective Codex settings");
    models[0].defaultReasoningEffort = "unsupported";
    expect(() =>
      resolveDemoModelSelection({ ...explicit, effort: "" }, models, {}),
    ).toThrow("does not support");
    expect(
      resolveDemoModelSelection({ ...explicit, effort: "" }, models).selection
        .effort,
    ).toBe("high");
  });
});
