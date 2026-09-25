// @vitest-environment jsdom
import "./test-setup";
import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { SafeMarkdown } from "./SafeMarkdown";
afterEach(cleanup);
describe("untrusted Markdown", () => {
  it("drops remote, data, and reference images instead of requesting assets", () => {
    const { container } = render(
      <SafeMarkdown>
        {
          "![tracking](https://example.test/pixel.png)\n\n![data](data:image/svg+xml;base64,PHN2Zy8+)\n\n![reference][image]\n\n[image]: https://example.test/ref.png"
        }
      </SafeMarkdown>,
    );
    expect(
      container.querySelector("img, svg, iframe, video, audio, source"),
    ).toBeNull();
    expect(container.querySelector("[src]")).toBeNull();
  });
  it("renders Markdown links as plain text and leaves autolinking off", () => {
    const { container } = render(
      <SafeMarkdown>
        {
          "[Visible label](https://example.test) [unsafe](javascript:alert(1))\n\nhttps://example.test/bare user@example.test <https://example.test/angle>"
        }
      </SafeMarkdown>,
    );
    expect(screen.getByText(/Visible label/)).toBeTruthy();
    expect(container.querySelector('a, [role="link"]')).toBeNull();
  });
  it("never turns inline or block HTML into elements", () => {
    const { container } = render(
      <SafeMarkdown>
        {
          'Hello <img src="https://example.test/pixel" onerror="alert(1)"> <a href="https://example.test">HTML link</a> <script>alert(1)</script>\n\n<iframe src="https://example.test"></iframe>'
        }
      </SafeMarkdown>,
    );
    expect(
      container.querySelector("img, a, script, iframe, [onerror]"),
    ).toBeNull();
    expect(container.textContent).toContain("Hello");
  });
});
