import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "gaussdataflow",
  description:
    "Open-source data integration — Airbyte-protocol compatible, built in Rust and Next.js",
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
