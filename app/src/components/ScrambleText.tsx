import { useEffect, useRef } from "react";
import { animate, scrambleText, utils } from "animejs";
import { useI18n } from "../i18n";

type TextSink = { textContent: string | null };

// 字符 scramble 文本:text 变化时用 anime.js 从乱码 settle 到目标文本,首次挂载不动画。
export function ScrambleText({
  text,
  className,
  cursor = "░▒▓",
}: {
  text: string;
  className?: string;
  cursor?: string | false;
}) {
  const { lang } = useI18n();
  const ref = useRef<HTMLSpanElement>(null);
  const lastText = useRef<string | null>(null);
  const lastLang = useRef(lang);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (lastText.current === null) {
      el.textContent = text;
      lastText.current = text;
      lastLang.current = lang;
      return;
    }
    // 语言切换直接写入、不播 scramble,避免整屏标题/状态条同时跳动。
    if (lastLang.current !== lang) {
      lastLang.current = lang;
      lastText.current = text;
      el.textContent = text;
      return;
    }
    if (lastText.current === text) return;
    lastText.current = text;
    // Animate on a plain object and copy to textContent so untrusted device names
    // never pass through HTML parsing.
    const animationTarget: TextSink = { textContent: el.textContent ?? "" };
    animate(animationTarget, {
      textContent: scrambleText({
        text,
        from: "center",
        duration: 520,
        cursor,
        ease: "inOut",
        override: false,
      }),
      onUpdate: () => {
        el.textContent = animationTarget.textContent ?? "";
      },
      onComplete: () => {
        el.textContent = text;
      },
    } as never);
    return () => {
      utils.remove(animationTarget);
      el.textContent = text;
    };
  }, [text, cursor, lang]);

  return <span ref={ref} className={className} />;
}
