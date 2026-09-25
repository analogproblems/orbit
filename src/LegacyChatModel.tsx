import { useState } from "react";
import { Modal } from "./Modal";
import { useOrbit } from "./store";
import {
  chatModelUnavailable,
  defaultEffort,
  selectionError,
  type Review,
} from "./types";

export function LegacyChatModel({
  review,
  onClose,
}: {
  review: Review;
  onClose: () => void;
}) {
  const { snapshot: s, action } = useOrbit();
  const [selection, setSelection] = useState(() =>
    !selectionError(s.models, s.settings)
      ? { model: s.settings.model, effort: s.settings.effort }
      : { model: "", effort: "" },
  );
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const model = s.models.find((m) => m.model === selection.model);
  const unavailable =
    chatModelUnavailable(s, review) || selectionError(s.models, selection);
  return (
    <Modal
      title="Choose chat model"
      onClose={() => {
        if (!saving) onClose();
      }}
      error={error}
    >
      <p>
        This one-time choice pins the model and effort for future messages in
        this chat. The original report's model information stays unchanged.
      </p>
      <div className="form-grid">
        <label>
          Chat model
          <select
            value={selection.model}
            disabled={saving}
            onChange={(e) => {
              const model = s.models.find((m) => m.model === e.target.value)!;
              setSelection({
                model: model.model,
                effort: defaultEffort(model) ?? "",
              });
            }}
          >
            <option value="" disabled>
              Choose a model
            </option>
            {s.models.map((m) => (
              <option key={m.model} value={m.model}>
                {m.displayName}
              </option>
            ))}
          </select>
        </label>
        <label>
          Chat reasoning effort
          <select
            value={selection.effort}
            disabled={saving || !model}
            onChange={(e) =>
              setSelection({ ...selection, effort: e.target.value })
            }
          >
            <option value="" disabled>
              Choose an effort
            </option>
            {model?.supportedReasoningEfforts.map((e) => (
              <option key={e.reasoningEffort} value={e.reasoningEffort}>
                {e.reasoningEffort}
              </option>
            ))}
          </select>
        </label>
      </div>
      {unavailable && <p className="action-notice">{unavailable}</p>}
      <div className="modal-actions">
        <button
          className="button secondary"
          disabled={saving}
          onClick={onClose}
        >
          Cancel
        </button>
        <button
          className="button primary"
          disabled={saving || !!unavailable}
          onClick={async () => {
            setSaving(true);
            setError(null);
            const result = await action(
              "set_legacy_chat_model",
              { reviewId: review.id, ...selection },
              { quiet: true },
            );
            if (result.ok) onClose();
            else setError(result.error);
            setSaving(false);
          }}
        >
          {saving ? "Saving…" : "Save chat model"}
        </button>
      </div>
    </Modal>
  );
}
