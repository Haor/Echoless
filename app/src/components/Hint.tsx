import { useRef, useState, type ReactNode } from "react";

// 自绘悬浮提示:单色直角,匹配主题;短延迟弹出(比原生 title 快)。
export function Hint({
  text,
  children,
}: {
  text?: string;
  children: ReactNode;
}) {
  const [show, setShow] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  if (!text) return <>{children}</>;
  return (
    <span
      className="hint"
      onMouseEnter={() => {
        timer.current = window.setTimeout(() => setShow(true), 240);
      }}
      onMouseLeave={() => {
        clearTimeout(timer.current);
        setShow(false);
      }}
    >
      {children}
      {show && <span className="hint-pop">{text}</span>}
    </span>
  );
}
