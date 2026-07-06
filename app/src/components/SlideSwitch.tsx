import { ScrambleText } from "./ScrambleText";

// 物理滑动开关:主体方块在条纹轨道里左右滑动 + 标签 scramble。
// 首页主开关与 Diagnostics 录制共用。
export function SlideSwitch({
  on,
  onToggle,
  disabled,
  small,
  onLabel = "ON",
  offLabel = "OFF",
}: {
  on: boolean;
  onToggle: () => void;
  disabled?: boolean;
  small?: boolean;
  onLabel?: string;
  offLabel?: string;
}) {
  return (
    <button
      type="button"
      className={`power ${on ? "on" : "off"} ${small ? "sm" : ""}`}
      disabled={disabled}
      onClick={onToggle}
    >
      <span className="slider">
        {/* ON/OFF 是短文本且切后长期不变,reveal 前沿残留无后续动画覆盖会
            持久可见(文字上方两条线)—— 这里关掉前沿字符。 */}
        <ScrambleText text={on ? onLabel : offLabel} cursor={false} />
      </span>
    </button>
  );
}
