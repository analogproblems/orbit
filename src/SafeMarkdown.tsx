import { memo, type ReactNode } from "react";
import { Markdown } from "@astryxdesign/core";

// All PR and agent text is untrusted. Astryx renders HTML as text; keep its
// autolinking opt-in unset and suppress image fetching and clickable links.
const components = {
  image: () => null,
  link: ({ children }: { children: ReactNode }) => <>{children}</>,
};
export const SafeMarkdown = memo(function SafeMarkdown({
  children,
  isStreaming = false,
}: {
  children: string;
  isStreaming?: boolean;
}) {
  return (
    <Markdown
      components={components}
      headingLevelStart={3}
      isStreaming={isStreaming}
      contentWidth="100%"
    >
      {children}
    </Markdown>
  );
});
