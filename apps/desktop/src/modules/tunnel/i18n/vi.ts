/**
 * What the Tunnel module calls everything, in Vietnamese.
 *
 * Pure data, importing nothing from `src/i18n/`: `dicts.ts` imports this file, so anything
 * importing back out of there would close a cycle.
 */
const tunnelVi = {
  tunnelTab: {
    newTabTitle: "Tunnel",
    settingsTitle: "Tunnel",
    title: "Tunnel",
    description: "Đưa một địa chỉ trên máy này ra internet qua quick tunnel của Cloudflare.",
    address: "Địa chỉ trên máy",
    addressPlaceholder: "localhost:5173",
    start: "Bật",
    stop: "Tắt",
    copy: "Chép",
    copied: "Đã chép",
    open: "Mở",
    anyoneWithTheLink: "Ai có link đều mở được khi tunnel còn chạy.",
    forTesting: "Quick tunnel chỉ để thử. Cloudflare không cam kết luôn chạy, và Server-Sent Events không dùng được.",
    running: "Tunnel",
    stateConnecting: "Đang kết nối…",
    stateOpen: "Đang chạy",
    stateFailed: "Đã dừng",
    empty: "Không có tunnel nào đang chạy.",
    columnAddress: "Địa chỉ",
    columnState: "Trạng thái",
    columnUrl: "URL công khai",
    needCloudflared: "Tunnel chạy bằng cloudflared, mà máy này chưa có.",
    download: "Tải cloudflared",
    downloadSize: "cloudflared {{version}}, {{size}}",
    downloading: "Đang tải…",
    using: "Đang dùng {{path}}",
    hintAllowedHosts: "Dev server từ chối địa chỉ của tunnel. Thêm nó vào server.allowedHosts trong cấu hình Vite.",
    hintTryIpv4: "Không có gì trả lời ở ::1. Thử 127.0.0.1 thay cho localhost.",
    pathLabel: "Đường dẫn cloudflared",
    pathHint: "Để trống thì dùng bản trong PATH, hoặc bản MixLab tải về.",
  },
  error: {
    tunnelTargetInvalid: "{{target}} không phải một địa chỉ.",
    tunnelTargetNotLocal: "{{target}} không nằm trên máy này. Tunnel chỉ chia sẻ localhost, 127.0.0.1 hoặc ::1.",
    tunnelNoBinary: "Chưa có cloudflared, cần tải về trước.",
    tunnelNoDownload: "cloudflared không có bản tải cho hệ điều hành này.",
    tunnelCannotStart: "cloudflared không chạy được: {{message}}",
  },
};

export default tunnelVi;
