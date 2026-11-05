import { useEffect, useMemo, useRef, useState } from "react";
import { PAGES, type PageKey } from "./pages";

type Props = {
  open: boolean;
  onSelect: (key: PageKey) => void;
};

/** 命令面板：当前只做导航；写操作按规格不进面板。Esc 由壳统一处理 */
export function CommandPalette({ open, onSelect }: Props) {
  const [query, setQuery] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  // 每次打开重置关键词并聚焦输入框；关着时不碰 DOM
  useEffect(() => {
    if (!open) return;
    setQuery("");
    inputRef.current?.focus();
  }, [open]);

  const matches = useMemo(() => {
    const keyword = query.trim().toLowerCase();
    return PAGES.filter(
      (page) =>
        !keyword ||
        page.label.toLowerCase().includes(keyword) ||
        page.description.toLowerCase().includes(keyword),
    );
  }, [query]);

  if (!open) return null;

  return (
    <div className="fixed inset-0 z-50 flex justify-center bg-foreground/20 pt-32">
      <div
        role="dialog"
        aria-modal="true"
        aria-label="命令面板"
        className="h-fit w-[34rem] max-w-[calc(100vw-2rem)] overflow-hidden rounded-xl border border-border bg-popover shadow-lg"
      >
        <input
          ref={inputRef}
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="跳到…"
          aria-label="搜索页面"
          className="h-11 w-full border-b border-border bg-transparent px-3 text-ui outline-none placeholder:text-muted-foreground"
        />
        <ul className="p-1">
          {matches.map(({ key, label, icon: Icon }) => (
            <li key={key}>
              <button
                type="button"
                onClick={() => onSelect(key)}
                className="flex h-9 w-full items-center gap-2.5 rounded-md px-2.5 text-ui hover:bg-muted"
              >
                <Icon
                  className="size-4 text-muted-foreground"
                  strokeWidth={1.75}
                  aria-hidden="true"
                />
                {label}
              </button>
            </li>
          ))}
          {matches.length === 0 ? (
            <li className="px-2.5 py-2 text-ui-sm text-muted-foreground">
              没有匹配项
            </li>
          ) : null}
        </ul>
      </div>
    </div>
  );
}
