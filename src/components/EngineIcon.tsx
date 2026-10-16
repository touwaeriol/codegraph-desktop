import codegraph from "@/assets/engines/codegraph.svg";
import serena from "@/assets/engines/serena.png";
import warpgrep from "@/assets/engines/warpgrep.png";
import { ScanSearch } from "lucide-react";

/** Official project icons, bundled from each project's GitHub repository or organization. */
const ICONS = { codegraph, serena, warpgrep } as const;
export type EngineName = keyof typeof ICONS | "rea";

export function EngineIcon({
  engine,
  size = 20,
}: {
  engine: EngineName;
  size?: number;
}) {
  if (engine === "rea") {
    return (
      <ScanSearch className="engine-icon" size={size} aria-hidden="true" />
    );
  }
  return (
    <img
      className="engine-icon"
      src={ICONS[engine]}
      width={size}
      height={size}
      alt=""
      aria-hidden="true"
      draggable={false}
    />
  );
}
