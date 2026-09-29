import { useCallback, useEffect, useState } from "react";

import Button from "../../../../components/Button";
import Card from "../../../../components/Card";
import ErrorBanner from "../../../../components/ErrorBanner";
import Input from "../../../../components/Input";
import RadioCard from "../../../../components/RadioCard";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import styles from "./IdlePanel.module.css";

type Choice = "recipe" | "never" | "minutes";

/** Three states, not two: absent (follow the recipe), 0 (off entirely), n (n minutes). Not a
 *  checkbox. */
export default function IdlePanel({ service }: { service: string }) {
  const [choice, setChoice] = useState<Choice>("recipe");
  const [minutes, setMinutes] = useState("30");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const { t } = useTranslation();

  const reload = useCallback(async () => {
    try {
      const current = (await api.serviceIdle(service)) as { minutes?: number | null };
      if (current.minutes === null || current.minutes === undefined) setChoice("recipe");
      else if (current.minutes === 0) setChoice("never");
      else {
        setChoice("minutes");
        setMinutes(String(current.minutes));
      }
      setError("");
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }, [service, t]);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function save() {
    setSaving(true);
    setError("");
    try {
      const value = choice === "recipe" ? undefined : choice === "never" ? 0 : Number(minutes);
      await api.serviceSetIdle({ service, minutes: value });
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <Card
      headingLevel={3}
      title={t("mixengine.servicesDetail.idle.title")}
      description={t("mixengine.servicesDetail.idle.about")}
    >
      {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}

      <div className={styles.choices}>
        <RadioCard
          name={`idle-${service}`}
          checked={choice === "recipe"}
          disabled={saving}
          onChange={() => setChoice("recipe")}
          label={t("mixengine.servicesDetail.idle.useRecipe")}
        />
        <RadioCard
          name={`idle-${service}`}
          checked={choice === "never"}
          disabled={saving}
          onChange={() => setChoice("never")}
          label={t("mixengine.servicesDetail.idle.never")}
        />
        <RadioCard
          name={`idle-${service}`}
          checked={choice === "minutes"}
          disabled={saving}
          onChange={() => setChoice("minutes")}
          label={t("mixengine.servicesDetail.idle.afterMinutes")}
        >
          <Input
            type="number"
            mono
            className={styles.minutes}
            aria-label={t("mixengine.servicesDetail.idle.afterMinutes")}
            value={minutes}
            disabled={saving || choice !== "minutes"}
            onChange={(e) => setMinutes(e.target.value)}
          />
        </RadioCard>
      </div>

      <div className={styles.actions}>
        <Button variant="primary" onClick={() => void save()} busy={saving ? t("mixengine.servicesDetail.idle.save") : undefined}>
          {t("mixengine.servicesDetail.idle.save")}
        </Button>
      </div>
    </Card>
  );
}