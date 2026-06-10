"use client";

import { useCallback, useEffect, useState } from "react";

export function StatusBadge({ status }: { status: string }) {
  const tone =
    {
      succeeded: "done",
      active: "done",
      complete: "done",
      running: "next",
      pending: "next",
      failed: "err",
      cancelled: "later",
      inactive: "later",
      deprecated: "later",
    }[status] ?? "later";
  return <span className={`badge ${tone}`}>{status}</span>;
}

export function ErrorNote({ error }: { error: string | null }) {
  if (!error) return null;
  return <p className="error-note">{error}</p>;
}

/** Fetch with optional polling; re-runs when `deps` change. */
export function usePoll<T>(
  fn: () => Promise<T>,
  intervalMs: number | null,
  deps: unknown[] = [],
) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    fn().then(setData, (e: Error) => setError(e.message));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);

  useEffect(() => {
    refresh();
    if (intervalMs === null) return;
    const timer = setInterval(refresh, intervalMs);
    return () => clearInterval(timer);
  }, [refresh, intervalMs]);

  return { data, error, refresh };
}

export function timeAgo(iso: string | undefined): string {
  if (!iso) return "—";
  const seconds = (Date.now() - new Date(iso).getTime()) / 1000;
  if (seconds < 60) return `${Math.max(1, Math.floor(seconds))}s ago`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
  return `${Math.floor(seconds / 86400)}d ago`;
}
