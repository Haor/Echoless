import { ScrambleText } from "./ScrambleText";

// 物理滑动主开关:主体方块在条纹轨道里左右滑动 + 标签 scramble。
export function SlideSwitch({
  on,
  onToggle,
  disabled,
}: {
  on: boolean;
  onToggle: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      className={`power ${on ? "on" : "off"}`}
      disabled={disabled}
      onClick={onToggle}
    >
      <span className="slider">
        <ScrambleText text={on ? "ON" : "OFF"} cursor={false} />
      </span>
    </button>
  );
}
