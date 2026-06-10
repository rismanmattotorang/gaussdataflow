import type { Metadata } from "next";
import Link from "next/link";
import { ToastHost } from "@/components/ui";
import "./globals.css";

export const metadata: Metadata = {
  title: "Gauss-DataFlow",
  description:
    "Gauss-DataFlow by Gaussian Technologies — the data movement platform for the agentic era, built in Rust.",
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en">
      <body>
        <nav className="topbar">
          <Link href="/" className="brand">
            Gauss-<span>DataFlow</span>
          </Link>
          <Link href="/" className="navlink">
            Workspaces
          </Link>
          <Link href="/roadmap" className="navlink">
            Roadmap
          </Link>
          <a
            href="https://github.com/rismanmattotorang/gaussdataflow"
            className="navlink"
          >
            GitHub
          </a>
        </nav>
        {children}
        <ToastHost />
      </body>
    </html>
  );
}
