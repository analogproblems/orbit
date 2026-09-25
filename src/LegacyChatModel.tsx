import { useState } from "react";
import {
  Banner,
  Button,
  Dialog,
  DialogHeader,
  FormLayout,
  HStack,
  Layout,
  LayoutContent,
  Selector,
  Text,
  VStack,
} from "@astryxdesign/core";
import { useOrbit } from "./store";
import {
  chatModelUnavailable,
  defaultEffort,
  selectionError,
  type Review,
} from "./types";

export function LegacyChatModel({
  review,
  isOpen,
  onClose,
}: {
  review: Review;
  isOpen: boolean;
  onClose: () => void;
}) {
  const [saving, setSaving] = useState(false);
  const close = (open: boolean) => {
    if (!open && !saving) onClose();
  };
  return (
    <Dialog
      isOpen={isOpen}
      onOpenChange={close}
      purpose={saving ? "required" : "form"}
      width={520}
    >
      <Layout
        header={
          <DialogHeader
            title="Choose chat model"
            onOpenChange={saving ? undefined : close}
          />
        }
        content={
          <LayoutContent>
            {isOpen && (
              <ChatModelForm
                review={review}
                saving={saving}
                setSaving={setSaving}
                onClose={onClose}
              />
            )}
          </LayoutContent>
        }
      />
    </Dialog>
  );
}

function ChatModelForm({
  review,
  saving,
  setSaving,
  onClose,
}: {
  review: Review;
  saving: boolean;
  setSaving: (saving: boolean) => void;
  onClose: () => void;
}) {
  const { snapshot: s, action } = useOrbit();
  const [selection, setSelection] = useState(() =>
    !selectionError(s.models, s.settings)
      ? { model: s.settings.model, effort: s.settings.effort }
      : { model: "", effort: "" },
  );
  const [error, setError] = useState<string | null>(null);
  const model = s.models.find((m) => m.model === selection.model);
  const unavailable =
    chatModelUnavailable(s, review) || selectionError(s.models, selection);
  return (
    <VStack gap={4}>
      <Text>
        This one-time choice pins the model and effort for future messages in
        this chat. The original report's model information stays unchanged.
      </Text>
      {error && <Banner status="error" title={error} />}
      <FormLayout>
        <Selector
          label="Chat model"
          value={selection.model}
          isDisabled={saving}
          placeholder="Choose a model"
          options={s.models.map((m) => ({
            value: m.model,
            label: m.displayName,
          }))}
          onChange={(value) => {
            const model = s.models.find((m) => m.model === value)!;
            setSelection({
              model: model.model,
              effort: defaultEffort(model) ?? "",
            });
          }}
        />
        <Selector
          label="Chat reasoning effort"
          value={selection.effort}
          isDisabled={saving || !model}
          placeholder="Choose an effort"
          options={
            model?.supportedReasoningEfforts.map((e) => e.reasoningEffort) ?? []
          }
          onChange={(effort) => setSelection({ ...selection, effort })}
        />
      </FormLayout>
      {unavailable && (
        <Text role="status" color="secondary">
          {unavailable}
        </Text>
      )}
      <HStack hAlign="end" gap={2}>
        <Button label="Cancel" isDisabled={saving} onClick={onClose} />
        <Button
          label={saving ? "Saving…" : "Save chat model"}
          variant="primary"
          isLoading={saving}
          isDisabled={saving || !!unavailable}
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
        />
      </HStack>
    </VStack>
  );
}
