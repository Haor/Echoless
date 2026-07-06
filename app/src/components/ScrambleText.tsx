import { useEffect, useRef } from "react";
import { animate, scrambleText, utils } from "animejs";

// 字符 scramble 文本:text 变化(或 trigger 变化)时,用 anime.js 把内容
// 从乱码 ░▒▓ settle 到目标文本,避免硬切。首次挂载不动画。
export function ScrambleText({
  text,
  trigger,
  className,
  cursor = "░▒▓",
}: {
  text: string;
  trigger?: unknown;
  className?: string;
  // reveal 波前沿字符。短文本(如 POWER 的 ON/OFF)+ text 长期不变时,前沿
  // 残留没有后续 scramble 覆盖会持久可见 —— 这类场景传 false 关掉前沿。
  cursor?: string | false;
}) {
  const ref = useRef<HTMLSpanElement>(null);
  const lastText = useRef<string | null>(null);
  const lastTrig = useRef<unknown>(undefined);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    // 首次:直接写入,不动画(也避开 StrictMode 双调用的误触发)
    if (lastText.current === null) {
      el.textContent = text;
      lastText.current = text;
      lastTrig.current = trigger;
      return;
    }
    if (lastText.current === text && lastTrig.current === trigger) return;
    lastText.current = text;
    lastTrig.current = trigger;
    // 先停掉仍在飞的上一个 scramble:两个 override:false 动画并存会同时改写
    // 同一元素的 innerHTML,settle 后残留错位的乱码结构(表现为文字上方残留
    // 两条线)。参照 VolumeWheel 的做法先 remove 再启新动画。
    utils.remove(el);
    animate(el, {
      // scrambleText 作为 innerHTML 的目标值(anime.js v4 文本插件)
      innerHTML: scrambleText({
        text,
        from: "center",
        duration: 520,
        cursor,
        ease: "inOut",
        override: false,
      }),
      // 收尾兜底:强制写回纯文本,清掉任何残留的 scramble 结构。
      onComplete: () => {
        el.textContent = text;
      },
    } as never);
    return () => {
      // 中断 / 卸载路径也要收敛到纯文本:scrambleText 只在动画跑到进度 1 的那帧
      // 才写回 settledText,若动画在中途被停(下次切换、组件卸载、clock 暂停),
      // innerHTML 会永久滞留在含 cursor/乱码的中间态,再没有帧刷新它。补一刀根除。
      utils.remove(el);
      el.textContent = text;
    };
  }, [text, trigger, cursor]);

  return <span ref={ref} className={className} />;
}
