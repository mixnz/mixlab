/**
 * Nhóm package theo chức năng cho tab con — thuần hiển thị, `package.*` không có field nào nói
 * chuyện này (xem `PackageSummary`/`PackageRelease`).
 *
 * `"other"` là chốt cho mọi tên chưa biết, không phải một trường hợp lỗi: một package MixEngine
 * thêm sau này (registry mới hơn bản MixDB đang chạy) vẫn phải hiện ra ở đâu đó, không được lặng
 * lẽ biến mất vì bảng tra không có tên nó.
 */
export type PackageCategory = "web" | "database" | "cache" | "other";

/** Thứ tự tab con luôn cố định, không phụ thuộc thứ tự `package.list_available` trả về. */
export const PACKAGE_CATEGORY_ORDER: PackageCategory[] = ["web", "database", "cache", "other"];

const WEB_SERVERS = new Set(["caddy", "nginx", "apache", "apache2", "httpd"]);
const DATABASES = new Set(["mariadb", "mysql", "postgres", "postgresql", "mongodb", "mongo", "sqlite"]);
const CACHES = new Set(["redis", "memcached"]);

export function packageCategory(packageName: string): PackageCategory {
  const key = packageName.toLowerCase();
  if (WEB_SERVERS.has(key)) return "web";
  if (DATABASES.has(key)) return "database";
  if (CACHES.has(key)) return "cache";
  return "other";
}
