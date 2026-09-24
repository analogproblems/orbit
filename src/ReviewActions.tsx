import { useState } from "react";
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
    <div className="review-actions">
      {review.status === "posted" && review.commentUrl && (
        <button
          className="text-button"
          onClick={() => {
            if (desktop) void action("open_github", { url: review.commentUrl });
            else
              window.open(review.commentUrl, "_blank", "noopener,noreferrer");
          }}
        >
          View comment on GitHub <ArrowUpRight size={13} />
        </button>
      )}
      {review.status === "posted" && !review.commentUrl && (
        <span>Comment published</span>
      )}
      {exportable && canExport(review) && (
        <button
          className="text-button"
          disabled={exporting}
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
            else if (result.data) setNotice(`Report exported: ${result.data}`);
          }}
        >
          <Download size={13} />
          {exporting ? "Exporting…" : "Export report"}
        </button>
      )}
      {notice && <span role="status">{notice}</span>}
      {error && <span role="alert">{error}</span>}
    </div>
  );
}
