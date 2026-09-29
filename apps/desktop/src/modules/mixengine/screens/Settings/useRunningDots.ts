import { useEffect, useState } from "react";

/** Running dots `""` → `"."` → `".."` → `"..."`, repeating — a sign of life while a long task is
 *  running. */
export function useRunningDots(active: boolean): string {
  const [count, setCount] = useState(0);
  useEffect(() => {
    if (!active) {
      setCount(0);
      return;
    }
    const id = window.setInterval(() => setCount((n) => (n + 1) % 4), 450);
    return () => window.clearInterval(id);
  }, [active]);
  return ".".repeat(count);
}
