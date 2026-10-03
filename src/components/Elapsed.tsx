import { t, useLanguage } from "@/lib/i18n";
import { useEffect, useState } from "react";
export function Elapsed({ since }: { since: string }) {
  useLanguage();
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);
  const seconds = Math.max(
    0,
    Math.floor((now - new Date(since).getTime()) / 1000),
  );
  return (
    <span className="mono text-sm">
      {Math.floor(seconds / 3600)}
      {t("时")}
      {Math.floor(seconds / 60) % 60}
      {t("分")} {seconds % 60}
      {t("秒")}
    </span>
  );
}
