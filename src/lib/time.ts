/** Format an ISO/UTC timestamp as local date + time; falls back to the raw
 * string when it can't be parsed (so a bad value is visible, not silently ""). */
export function formatDateTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}
