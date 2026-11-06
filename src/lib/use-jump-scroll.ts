import { useLayoutEffect, useRef } from "react";

/** 定位跳转的请求：目标行键 + 递增序号（同一键再次跳转也要重新触发） */
export type FocusTarget = { key: string; seq: number };

type JumpScrollOptions = {
  /** 当前选中行的稳定键；数据到齐或选中变化后把它滚进视口 */
  activeKey: string | null | undefined;
  /** 定位跳转的序号：同一个键第二次跳也要重新触发定位 */
  seq: number;
  /** 定位跳转的目标键：只有它成为选中行才居中，点击行仍按最小滚动 */
  jumpKey: string | null | undefined;
};

/**
 * 选中行的滚动契约：命令面板跳转居中，点击行保持最小滚动、已可见时
 * 不动。绘制前执行，键盘焦点不动。返回挂在当前选中行上的 ref。
 */
export function useJumpScroll<T extends HTMLElement>({
  activeKey,
  seq,
  jumpKey,
}: JumpScrollOptions) {
  const row = useRef<T | null>(null);
  const appliedSeq = useRef(0);

  useLayoutEffect(() => {
    if (!activeKey) return;
    const jumped =
      seq !== 0 && seq !== appliedSeq.current && activeKey === jumpKey;
    if (jumped) appliedSeq.current = seq;
    row.current?.scrollIntoView({ block: jumped ? "center" : "nearest" });
  }, [activeKey, seq, jumpKey]);

  return row;
}
