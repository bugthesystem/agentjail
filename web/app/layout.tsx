import "./globals.css";
import type { ReactNode } from "react";
import type { Metadata } from "next";
import { headers } from "next/headers";
import { Sidebar } from "@/components/Sidebar";

export const metadata: Metadata = {
  title: "agentjail",
  description: "Phantom-token sandbox control plane",
};

export default async function RootLayout({ children }: { children: ReactNode }) {
  const h = await headers();
  const pathname = h.get("x-invoke-path") ?? h.get("x-pathname") ?? "/";
  return (
    <html lang="en">
      <body>
        <div className="flex min-h-screen">
          <Sidebar active={pathname} />
          <main className="flex-1 px-8 py-6">{children}</main>
        </div>
      </body>
    </html>
  );
}
