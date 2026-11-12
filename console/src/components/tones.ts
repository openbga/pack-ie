import {
  CircleCheck,
  CircleMinus,
  CirclePause,
  Info,
  type LucideIcon,
  OctagonX,
  TriangleAlert,
} from "lucide-react";

/**
 * Semantic tones. Colors come from `[data-tone]` in global.css; components pair each tone
 * with its icon and a text label, so status never relies on color alone.
 */
export type Tone = "info" | "success" | "warning" | "danger" | "hold" | "neutral";

export const toneIcons: Record<Tone, LucideIcon> = {
  info: Info,
  success: CircleCheck,
  warning: TriangleAlert,
  danger: OctagonX,
  hold: CirclePause,
  neutral: CircleMinus,
};
