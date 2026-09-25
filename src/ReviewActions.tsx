import { useState } from "react";
import { Banner, Button, HStack, Text, VStack } from "@astryxdesign/core";
import { ArrowUpRight, Download } from "lucide-react";
import { useOrbit, desktop } from "./store";
import { canExport, type Review } from "./types";

export function ReviewActions({
  review,
  exportable = true,
}: {
  review: Review;
  exportable?: boolean;
}) {
  const action = useOrbit((state) => state.action);
  const [exporting, setExporting] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  return (
    <VStack gap={2}>
      <HStack gap={2} wrap="wrap">
        {review.status === "posted" && review.commentUrl && (
          <Button
            label="View comment on GitHub"
            variant="ghost"
            size="sm"
            icon={<ArrowUpRight />}
            onClick={() => {
              if (desktop)
                void action("open_github", { url: review.commentUrl });
              else
                window.open(review.commentUrl, "_blank", "noopener,noreferrer");
            }}
          />
        )}
        {review.status === "posted" && !review.commentUrl && (
          <Text>Comment published</Text>
        )}
        {exportable && canExport(review) && (
          <Button
            label={exporting ? "Exporting…" : "Export report"}
            variant="ghost"
            size="sm"
            icon={<Download />}
            isLoading={exporting}
            onClick={async () => {
              setExporting(true);
              setNotice(null);
              setError(null);
              const result = await action<string | null>(
                "export_report",
                { reviewId: review.id },
                { quiet: true },
              );
              setExporting(false);
              if (!result.ok) setError(result.error);
              else if (result.data)
                setNotice(`Report exported: ${result.data}`);
            }}
          />
        )}
      </HStack>
      {notice && <Text role="status">{notice}</Text>}
      {error && <Banner status="error" title={error} />}
    </VStack>
  );
}
