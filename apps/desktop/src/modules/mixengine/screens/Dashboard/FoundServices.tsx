import { useCallback, useEffect, useState } from "react";

import Button from "../../../../components/Button";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import { rowsFrom, type FoundRow } from "../../foundServices";
import styles from "./FoundServices.module.css";
import FoundServicesDialog from "./FoundServicesDialog";

/**
 * Service data an earlier install left under `data/` — roadmap task T182g.
 *
 * Drawn only while `service.found` answers rows, and gone once nothing is left, like `PathNudge`:
 * no dismiss button and no stored flag. **Nothing starts on its own**: an adopted service comes back
 * stopped, and starting it is the Services screen's.
 *
 * Read again each time Dashboard comes back to the front, for `PathNudge`'s reason — Dashboard stays
 * mounted, and an adopt from `mix` in a terminal would otherwise never reach this card. A failed
 * read is silent: a red Dashboard over a reminder is a red Dashboard over nothing.
 */
export default function FoundServices({
  active,
  onAdopted,
}: {
  active: boolean;
  /** Called after an adopt, so the services table picks up the new row. */
  onAdopted: () => void;
}) {
  const [rows, setRows] = useState<FoundRow[]>([]);
  const [open, setOpen] = useState(false);
  const [adopting, setAdopting] = useState<string | null>(null);
  const [adopted, setAdopted] = useState<string | null>(null);
  const [error, setError] = useState("");
  const { t } = useTranslation();

  const read = useCallback(() => {
    api
      .serviceFound()
      .then((list) => setRows(rowsFrom(list)))
      .catch(() => {
        // Keep what was there: not knowing changes nothing.
      });
  }, []);

  useEffect(() => {
    if (active) read();
  }, [active, read]);

  async function adopt(id: string) {
    setAdopting(id);
    setError("");
    try {
      await api.serviceAdopt(id);
      setAdopted(id);
      onAdopted();
      read();
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setAdopting(null);
    }
  }

  if (rows.length === 0 && !open) return null;

  return (
    <section className={styles.card}>
      <h3 className={styles.title}>{t("mixengine.foundServices.title")}</h3>
      <div className={styles.row}>
        <p className={styles.intro}>{t("mixengine.foundServices.intro", { count: rows.length })}</p>
        <Button variant="soft" onClick={() => setOpen(true)}>
          {t("mixengine.foundServices.review")}
        </Button>
      </div>

      {open && (
        <FoundServicesDialog
          rows={rows}
          adopting={adopting}
          adopted={adopted}
          error={error}
          onAdopt={(id) => void adopt(id)}
          onClose={() => {
            setOpen(false);
            setAdopted(null);
          }}
        />
      )}
    </section>
  );
}
