import { useState } from "react";
import Button from "../../../../components/Button";
import Input from "../../../../components/Input";
import Select, { type SelectOption } from "../../../../components/Select";
import { useTranslation } from "../../../../i18n";
import CopyField from "../../components/CopyField";
import { ID_KINDS, RANDOM_BYTES, nanoid, ulid, uuidv4, uuidv7, type IdKind } from "./ids";
import styles from "./Panel.module.css";

const MAX_COUNT = 1000;

/* Type names are not translated: `ULID` and `UUID v7` are the formats' own names, like
   `snake_case`. */
const KIND_LABEL: Record<IdKind, string> = {
  uuidv4: "UUID v4",
  uuidv7: "UUID v7",
  ulid: "ULID",
  nanoid: "NanoID",
};

const KIND_OPTIONS: SelectOption<IdKind>[] = ID_KINDS.map((kind) => ({
  value: kind,
  label: KIND_LABEL[kind],
}));

function IdsPanel() {
  const { t } = useTranslation();
  const [kind, setKind] = useState<IdKind>("uuidv4");
  const [count, setCount] = useState(10);
  const [result, setResult] = useState("");

  // Generated in the handler, not in render: the result has to stay put until the next click,
  // while render may run again at any moment.
  const generate = () => {
    const now = Date.now();
    const size = RANDOM_BYTES[kind];
    const out: string[] = [];
    for (let i = 0; i < count; i++) {
      const rnd = crypto.getRandomValues(new Uint8Array(size));
      out.push(
        kind === "uuidv4"
          ? uuidv4(rnd)
          : kind === "uuidv7"
            ? uuidv7(now, rnd)
            : kind === "ulid"
              ? ulid(now, rnd)
              : nanoid(rnd),
      );
    }
    setResult(out.join("\n"));
  };

  return (
    <div className={styles.panel}>
      <div className={styles.controls}>
        <Select
          value={kind}
          options={KIND_OPTIONS}
          onChange={setKind}
          ariaLabel={t("toolbox.ids.kind")}
          className={styles.kind}
        />
        <Input
          type="number"
          min={1}
          max={MAX_COUNT}
          value={count}
          // Clamped here rather than relying only on the input's `min`/`max`: typing by hand still
          // gets past them, and a pasted number could be anything.
          onChange={(event) =>
            setCount(Math.min(MAX_COUNT, Math.max(1, Math.floor(Number(event.target.value)) || 1)))
          }
          aria-label={t("toolbox.ids.count")}
          className={styles.count}
        />
        <Button variant="primary" onClick={generate}>
          {t("toolbox.ids.generate")}
        </Button>
      </div>

      <CopyField label={t("toolbox.output")} value={result} multiline />
    </div>
  );
}

export default IdsPanel;
