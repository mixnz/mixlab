import { useState } from "react";
import Input, { Textarea } from "../../../../components/Input";
import Select, { type SelectOption } from "../../../../components/Select";
import { useTranslation } from "../../../../i18n";
import CopyField from "../../components/CopyField";
import { toDockerArgs, toEnv } from "../env/env";
import {
  DEFAULT_PORT,
  parseConnectionString,
  toEnvPairs,
  toJdbc,
  toUri,
  type ConnectionFields,
  type DbKind,
} from "./connection";
import styles from "./Panel.module.css";

/** The labels are product names, so they are not translated. */
const KINDS: SelectOption<DbKind>[] = [
  { value: "mysql", label: "MySQL" },
  { value: "postgres", label: "PostgreSQL" },
  { value: "mongodb", label: "MongoDB" },
  { value: "redis", label: "Redis" },
];

const BLANK: ConnectionFields = {
  kind: "mysql",
  srv: false,
  host: "localhost",
  port: "",
  user: "",
  password: "",
  database: "",
  params: [],
};

function ConnectionPanel() {
  const { t } = useTranslation();
  /* `fields` is the single source of truth, and the paste box keeps its own state. If the paste box
     also read back from `fields`, editing one field would rewrite the paste box, and the cursor of
     the person typing would jump to the start. */
  const [text, setText] = useState("");
  const [unreadable, setUnreadable] = useState(false);
  const [fields, setFields] = useState<ConnectionFields>(BLANK);

  const paste = (value: string): void => {
    setText(value);
    if (value.trim() === "") {
      setUnreadable(false);
      return;
    }
    const parsed = parseConnectionString(value);
    setUnreadable(parsed === null);
    if (parsed) setFields(parsed);
  };

  const set = <K extends keyof ConnectionFields>(key: K, value: ConnectionFields[K]): void => {
    setFields((current) => ({ ...current, [key]: value }));
  };

  const jdbc = toJdbc(fields);
  const pairs = toEnvPairs(fields);

  return (
    <div className={styles.panel}>
      <label className={styles.field}>
        <span className={styles.fieldLabel}>{t("toolbox.input")}</span>
        <Textarea
          value={text}
          onChange={(event) => paste(event.target.value)}
          placeholder={t("toolbox.connection.paste")}
          maxRows={4}
        />
      </label>

      {unreadable ? <p className={styles.error}>{t("toolbox.connection.unreadable")}</p> : null}

      <div className={styles.grid}>
        <label className={styles.field}>
          <span className={styles.fieldLabel}>{t("toolbox.connection.kind")}</span>
          <Select
            value={fields.kind}
            options={KINDS}
            onChange={(kind) => set("kind", kind)}
            ariaLabel={t("toolbox.connection.kind")}
          />
        </label>
        <label className={styles.field}>
          <span className={styles.fieldLabel}>{t("toolbox.connection.host")}</span>
          <Input value={fields.host} onChange={(e) => set("host", e.target.value)} />
        </label>
        <label className={styles.field}>
          <span className={styles.fieldLabel}>{t("toolbox.connection.port")}</span>
          <Input
            value={fields.port}
            onChange={(e) => set("port", e.target.value)}
            placeholder={DEFAULT_PORT[fields.kind]}
            disabled={fields.srv}
          />
        </label>
        <label className={styles.field}>
          <span className={styles.fieldLabel}>{t("toolbox.connection.user")}</span>
          <Input value={fields.user} onChange={(e) => set("user", e.target.value)} />
        </label>
        <label className={styles.field}>
          <span className={styles.fieldLabel}>{t("toolbox.connection.password")}</span>
          <Input value={fields.password} onChange={(e) => set("password", e.target.value)} />
        </label>
        <label className={styles.field}>
          <span className={styles.fieldLabel}>{t("toolbox.connection.database")}</span>
          <Input value={fields.database} onChange={(e) => set("database", e.target.value)} />
        </label>
      </div>

      {fields.srv ? <p className={styles.note}>{t("toolbox.connection.srvNote")}</p> : null}

      <CopyField label={t("toolbox.connection.uri")} value={toUri(fields)} />
      {jdbc === null ? (
        <p className={styles.note}>{t("toolbox.connection.jdbcNone")}</p>
      ) : (
        <CopyField label={t("toolbox.connection.jdbc")} value={jdbc} />
      )}
      <CopyField label={t("toolbox.connection.env")} value={toEnv(pairs)} multiline />
      <CopyField label={t("toolbox.connection.docker")} value={toDockerArgs(pairs)} multiline />
    </div>
  );
}

export default ConnectionPanel;
