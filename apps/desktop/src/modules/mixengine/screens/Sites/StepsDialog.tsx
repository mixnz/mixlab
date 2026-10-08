import Modal, { ModalBody } from "../../../../components/Modal";
import { useTranslation } from "../../../../i18n";
import type { ProjectDetail } from "@mixengine/api";
import NextStepsPanel from "../../components/NextStepsPanel";

interface Props {
  /** The site's project, as `project.show` read it. */
  detail: ProjectDetail;
  /** The address of the site the steps were asked from, for `open` rows. */
  url: string;
  terminalVisible: boolean;
  onClose: () => void;
}

/** What a site's project says is left to run, from its blueprint — T205, D8. */
export default function StepsDialog({ detail, url, terminalVisible, onClose }: Props) {
  const { t } = useTranslation();
  return (
    <Modal
      title={t("mixengine.sites.nextSteps")}
      onClose={onClose}
      size="normal"
      actions={[{ kind: "cancel", label: t("mixengine.afterApply.close"), onClick: onClose }]}
    >
      {() => (
        <ModalBody>
          {detail.next_steps ? (
            <NextStepsPanel
              project={detail.project.name}
              root={detail.project.root}
              siteUrl={url}
              steps={detail.next_steps}
              terminalVisible={terminalVisible}
              titled={false}
            />
          ) : (
            <p>{t("mixengine.sites.noNextSteps")}</p>
          )}
        </ModalBody>
      )}
    </Modal>
  );
}
