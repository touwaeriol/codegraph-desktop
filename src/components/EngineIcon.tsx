import codegraph from "@/assets/engines/codegraph.svg";
import serena from "@/assets/engines/serena.png";
import warpgrep from "@/assets/engines/warpgrep.png";

/** Official project icons, bundled from each project's GitHub repository or organization. */
const ICONS = { codegraph, serena, warpgrep } as const;
export type EngineName = keyof typeof ICONS;

export function EngineIcon({
  engine,
  size = 20,
}: {
  engine: EngineName;
  size?: number;
}) {
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
