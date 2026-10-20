import ConfirmDialog from "../../../components/ConfirmDialog";
import { useTranslation } from "../../../i18n";
import { SECRET_MASK } from "../environments";
import type { SwapOffer } from "./usePasteRequest";

interface SwapDialogProps {
  swap: SwapOffer;
  envName: string;
  onConfirm: () => void;
  onCancel: () => void;
}

/** The offer to put the environment's variables back into a pasted command, with what each one
 *  would replace. */
function SwapDialog({ swap, envName, onConfirm, onCancel }: SwapDialogProps) {
  const { t } = useTranslation();
  return (
    <ConfirmDialog
      title={t("rest.swapTitle")}
      message={t("rest.swapMessage", { env: envName })}
      confirmLabel={t("rest.swapConfirm")}
      cancelLabel={t("rest.swapCancel")}
      onConfirm={onConfirm}
      onCancel={onCancel}
    >
      <ul className="rest-swap-list">
        {swap.found.map((item) => (
          <li key={item.name} className="rest-swap-row">
            <code className="rest-swap-name">{`{{${item.name}}}`}</code>
            {/* A secret's value is dots here for the same reason it is dots under the URL box:
                the question is which variable, and the answer never needs the credential. */}
            <span className="rest-swap-value">{item.secret ? SECRET_MASK : item.value}</span>
            <span className="rest-swap-count">
              {t("rest.swapCount", { count: item.count })}
            </span>
          </li>
        ))}
      </ul>
    </ConfirmDialog>
  );
}

export default SwapDialog;
