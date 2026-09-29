import { lazy } from "react";

import { EngineIcon } from "../../icons";
import type { ModuleDefinition } from "../../shell/module";

/* Nạp khi một tab của module này được mở lần đầu, không phải lúc khởi động — cùng lý do với bốn
   module kia. Icon và nhãn thì eager: chúng có mặt trên tab strip trước khi có tab nào loại này. */
/** MixEngine: môi trường web dev cục bộ chạy trên máy này, quản lý từ đây. */
export const mixengineModule: ModuleDefinition = {
  id: "mixengine",
  labelKey: "app.moduleMixEngine",
  Icon: EngineIcon,
  defaultTitleKey: "mixengine.newTabTitle",
  /* Một tab là hết. Tab này là bảng điều khiển của **một** daemon trên **một** máy: mở cái thứ hai
     không cho xem thêm gì cả, chỉ là hai bản sao cùng một trạng thái, cạnh nhau, và cái nào cũng có
     thể là cái người dùng vừa đọc lần trước. Khác hẳn bốn module kia, nơi mỗi tab là một kết nối,
     một phiên, một yêu cầu. */
  singleTab: true,
  Tab: lazy(() => import("./MixEngineTab")),
  /* Loaded when the tray's frame first draws this section — T168, T192. The main window never
     loads it. */
  TraySection: lazy(() => import("./tray/TraySection")),
};
