import type { MouseEvent, ReactNode } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";
import { desktop, message } from "@/lib/ipc";

export function ExternalLink({
  href,
  children,
  className,
}: {
  href: string;
  children: ReactNode;
  className?: string;
}) {
  function open(event: MouseEvent<HTMLAnchorElement>) {
    if (!desktop || event.button > 1) return;
    event.preventDefault();
    void openUrl(href).catch((error) => toast.error(message(error)));
  }

  return (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      className={className}
      onClick={open}
      onAuxClick={open}
    >
      {children}
    </a>
  );
}
