import ConfirmDialog from "../../../../components/ConfirmDialog";
import { useTranslation } from "../../../../i18n";
import { needLabel, type AskingStep } from "../../requirementStep";

interface Props {
  /** `php 8.4.24` — what the person clicked Install on. */
  name: string;
  step: AskingStep;
  /** Agreed to installing what MixEngine can install, then the version. */
  onInstall: () => void;
  /** Agreed to installing `version` instead. */
  onChoose: (version: string) => void;
  onCancel: () => void;
}

/**
 * The one question an install asks when the machine lacks something.
 *
 * Shared by the Languages and Packages screens so the two cannot word the same approval differently.
 */
export default function RequirementDialog({ name, step, onInstall, onChoose, onCancel }: Props) {
  const { t } = useTranslation();

  if (step.kind === "consent") {
    return (
      <ConfirmDialog
        title={t("mixengine.requirements.consentTitle", { name })}
        message={t("mixengine.requirements.consentMessage", { name, arch: step.arches.join(", ") })}
        confirmLabel={t("mixengine.requirements.consentConfirm")}
        onCancel={onCancel}
        onConfirm={onInstall}
      />
    );
  }

  const version = step.version;
  return (
    <ConfirmDialog
      title={t("mixengine.requirements.chooseTitle", { name })}
      message={t("mixengine.requirements.chooseMessage", {
        needs: step.needs.map(needLabel).join(", "),
        version,
      })}
      confirmLabel={t("mixengine.requirements.chooseConfirm", { version })}
      onCancel={onCancel}
      onConfirm={() => onChoose(version)}
    />
  );
}
