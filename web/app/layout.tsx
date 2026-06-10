import type { Metadata } from "next";
import Link from "next/link";
import "./globals.css";

export const metadata: Metadata = {
  title: "gaussdataflow",
  description:
    "The data movement platform for the agentic era — built in Rust.",
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en">
      <body>
        <nav className="topbar">
          <Link href="/" className="brand">
            gauss<span>dataflow</span>
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
      </body>
    </html>
  );
}
