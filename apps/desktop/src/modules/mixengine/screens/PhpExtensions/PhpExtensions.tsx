import { useCallback, useEffect, useMemo, useState } from "react";

import Button from "../../../../components/Button";
import Card from "../../../../components/Card";
import EmptyState from "../../../../components/EmptyState";
import ErrorBanner from "../../../../components/ErrorBanner";
import PageHeader from "../../../../components/PageHeader";
import Select from "../../../../components/Select";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { RuntimeSummary } from "@mixengine/api";
import type { RuntimeTarget } from "@mixengine/api";
import ExtensionsPanel from "../Packages/ExtensionsPanel";
import styles from "./PhpExtensions.module.css";

/**
 * Bật tắt extension của PHP — T118.
 *
 * **Không có method mới.** `runtime.list_extensions` và `runtime.set_extension` đã tồn tại từ T28,
 * và `ExtensionsPanel` đã vẽ chúng từ khi có màn Runtimes (nay là Packages). Thứ thiếu là *đường
 * tới đó*: nó nằm sau một hàng phiên bản phải mở ra ở một màn khi đó tên là Runtimes, bốn hàng phía
 * trên một mục sidebar tên là *Extensions* mà lại nói về một chuyện hoàn toàn khác.
 *
 * **Cùng một component, vẽ ở hai nơi**, không phải hai bản chép: mở từ Packages vẫn được, và bản
 * thứ hai sẽ là bản lệch đúng vào ngày một trong hai được sửa.
 *
 * **Không có PHP thì một câu và một nút**, không phải một bảng rỗng: bảng rỗng bắt người ta đoán
 * xem họ thiếu bước nào.
 */
export default function PhpExtensions({
  active,
  onInstallPhp,
}: {
  active: boolean;
  onInstallPhp: () => void;
}) {
  const [installed, setInstalled] = useState<RuntimeSummary[] | null>(null);
  const [version, setVersion] = useState("");
  const [error, setError] = useState("");
  const { t } = useTranslation();

  /**
   * Giữ nguyên tham chiếu chừng nào `version` chưa đổi.
   *
   * `ExtensionsPanel` có `target` trong deps của `reload`, nên một object literal dựng ngay trong
   * JSX là một `target` mới **mỗi lần render** — và `MixEngineTab` render lại toàn bộ pane đang
   * mounted mỗi lần đổi màn. Nghĩa là mỗi lượt chuyển màn tốn thêm một `runtime.list_extensions`
   * hỏi lại đúng thứ vừa hỏi.
   */
  const target = useMemo<RuntimeTarget>(() => ({ kind: "php", version }), [version]);

  const reload = useCallback(async () => {
    try {
      const listed = await api.runtimesInstalled("php");
      setInstalled(listed.runtimes);
      // Bản mặc định của home, vì đó là bản `php` trên terminal đang chạy; không có thì bản đầu.
      setVersion((current) => {
        if (listed.runtimes.some((runtime) => runtime.version === current)) return current;
        const preferred = listed.runtimes.find((runtime) => runtime.default) ?? listed.runtimes[0];
        return preferred?.version ?? "";
      });
      setError("");
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }, [t]);

  /**
   * Đọc lại mỗi khi quay lại màn này, không chỉ lúc mount — `MixEngineTab.pane` giữ mọi màn
   * mounted và chỉ ẩn đi, nên "đã mount" không có nghĩa là "vừa được xem".
   *
   * Đây là màn duy nhất từng bỏ qua giao kèo ấy, và cái giá đúng bằng một lỗi: gỡ bản PHP đang
   * chọn ở màn Packages thì `Select` ở đây vẫn giữ nguyên nó, và `ExtensionsPanel` hỏi daemon về
   * một runtime không còn tồn tại ("no such runtime: php …"). Luật chọn lại version khi bản đang
   * chọn biến mất đã nằm sẵn trong `reload()` — thứ thiếu chỉ là một lượt gọi nữa.
   */
  useEffect(() => {
    if (active) void reload();
  }, [active, reload]);

  return (
    <div className={`mixengine-page ${styles.screen}`}>
      {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}

      <PageHeader title={t("mixengine.phpExtensions.title")} description={t("mixengine.phpExtensions.intro")} />

      {installed !== null && installed.length === 0 ? (
        <Card>
          <EmptyState
            title={t("mixengine.phpExtensions.noPhp")}
            action={
              <Button variant="primary" onClick={onInstallPhp}>
                {t("mixengine.phpExtensions.installPhp")}
              </Button>
            }
          />
        </Card>
      ) : (
        version !== "" && (
          <ExtensionsPanel
            target={target}
            leading={
              <>
                <Select
                  className={styles.select}
                  value={version}
                  onChange={setVersion}
                  ariaLabel={t("mixengine.phpExtensions.version")}
                  options={(installed ?? []).map((runtime) => ({
                    value: runtime.version,
                    label: runtime.default
                      ? `PHP ${runtime.version} — ${t("mixengine.phpExtensions.isDefault")}`
                      : `PHP ${runtime.version}`,
                  }))}
                />
                <Button variant="link" onClick={onInstallPhp}>
                  {t("mixengine.phpExtensions.installAnother")}
                </Button>
              </>
            }
          />
        )
      )}
    </div>
  );
}