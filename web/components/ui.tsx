"use client";

import Link from "next/link";
import { useCallback, useEffect, useRef, useState } from "react";

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

/** Fetch with optional polling; re-runs when `deps` change.
 *
 * Correctness guarantees a naive interval lacks: errors clear on the next
 * success (no stale banners), out-of-order responses are dropped, polling
 * pauses while the tab is hidden and resumes (with an immediate fetch) when
 * it becomes visible again. */
export function usePoll<T>(
  fn: () => Promise<T>,
  intervalMs: number | null,
  deps: unknown[] = [],
) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const seq = useRef(0);

  const refresh = useCallback(() => {
    const ticket = ++seq.current;
    fn().then(
      (d) => {
        if (ticket === seq.current) {
          setData(d);
          setError(null);
        }
      },
      (e: Error) => {
        if (ticket === seq.current) setError(e.message);
      },
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);

  useEffect(() => {
    refresh();
    if (intervalMs === null) {
      return () => {
        seq.current++; // invalidate in-flight responses on unmount/dep change
      };
    }
    const timer = setInterval(() => {
      if (!document.hidden) refresh();
    }, intervalMs);
    const onVisible = () => {
      if (!document.hidden) refresh();
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      seq.current++;
      clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisible);
    };
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

export function fmtNum(n: number | undefined | null): string {
  if (n === undefined || n === null) return "—";
  return n.toLocaleString("en-US");
}

/** Elapsed time of a job: running jobs tick against now. */
export function duration(start?: string, end?: string): string {
  if (!start) return "—";
  const seconds = Math.max(
    0,
    ((end ? new Date(end).getTime() : Date.now()) -
      new Date(start).getTime()) /
      1000,
  );
  if (seconds < 60) return `${Math.floor(seconds)}s`;
  if (seconds < 3600)
    return `${Math.floor(seconds / 60)}m ${Math.floor(seconds % 60)}s`;
  return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
}

// ---- toasts -------------------------------------------------------------
// Module-level store so any page can `toast("…")` without context plumbing;
// <ToastHost /> in the root layout renders the stack.

type Toast = { id: number; text: string; tone: "ok" | "err" };
let nextToastId = 1;
let toastListener: ((t: Toast) => void) | null = null;

export function toast(text: string, tone: "ok" | "err" = "ok") {
  toastListener?.({ id: nextToastId++, text, tone });
}

export function ToastHost() {
  const [toasts, setToasts] = useState<Toast[]>([]);
  useEffect(() => {
    toastListener = (t) => {
      setToasts((prev) => [...prev.slice(-3), t]);
      setTimeout(
        () => setToasts((prev) => prev.filter((x) => x.id !== t.id)),
        5000,
      );
    };
    return () => {
      toastListener = null;
    };
  }, []);
  return (
    <div className="toast-stack" role="status" aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className={`toast ${t.tone}`}>
          {t.text}
        </div>
      ))}
    </div>
  );
}

// ---- shared layout pieces ----------------------------------------------

export function Breadcrumbs({
  items,
}: {
  items: { label: string; href?: string }[];
}) {
  return (
    <nav className="breadcrumbs">
      {items.map((item, i) => (
        <span key={i}>
          {i > 0 && <span className="sep">›</span>}
          {item.href ? <Link href={item.href}>{item.label}</Link> : item.label}
        </span>
      ))}
    </nav>
  );
}

export function StatCard({
  label,
  value,
  detail,
  tone,
}: {
  label: string;
  value: string;
  detail?: string;
  tone?: "ok" | "warn" | "err";
}) {
  return (
    <div className={`stat-card ${tone ?? ""}`}>
      <div className="stat-label">{label}</div>
      <div className="stat-value">{value}</div>
      {detail && <div className="stat-detail">{detail}</div>}
    </div>
  );
}
