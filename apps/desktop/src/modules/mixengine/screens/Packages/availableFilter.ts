/**
 * Lọc bảng "chưa cài" của Packages — thuần hiển thị, không có RPC nào nhận câu tìm kiếm này
 * (`runtime.list_available`/`package.list_available` trả về cả danh sách, xem `Languages.tsx`).
 *
 * Mỗi từ trong câu tìm phải khớp một chỗ nào đó, không phải cả câu khớp liền một mạch: gõ
 * `php 8.3` vẫn ra `php 8.3.14` dù hai mẩu đó nằm ở hai trường khác nhau, và thứ tự gõ không
 * quyết định kết quả.
 */
export function matchesAvailable(fields: string[], query: string): boolean {
  const words = query.toLowerCase().split(/\s+/).filter((word) => word !== "");
  if (words.length === 0) return true;
  const haystack = fields.join(" ").toLowerCase();
  return words.every((word) => haystack.includes(word));
}
