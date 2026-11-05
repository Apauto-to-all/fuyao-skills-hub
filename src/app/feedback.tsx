import type { ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { isGuiError } from "@/lib/ipc";

/** 居中提示：加载与空态共用，只负责排版 */
export function Centered({ children }: { children: ReactNode }) {
  return (
    <div className="flex h-full items-center justify-center px-6 text-ui-sm text-muted-foreground">
      {children}
    </div>
  );
}

type ErrorBlockProps = {
  error: unknown;
  onRetry: () => void;
};

/** 页内错误块：查询失败用它，锁冲突等写失败走 toast */
export function ErrorBlock({ error, onRetry }: ErrorBlockProps) {
  const message = isGuiError(error) ? error.message : String(error);
  const code = isGuiError(error) ? error.code : "unknown";

  return (
    <div
      role="alert"
      className="mx-auto flex max-w-md flex-col items-center gap-3 px-6 py-24 text-center"
    >
      <p className="text-ui font-medium">这一步没成功</p>
      <p className="text-ui-sm text-muted-foreground">{message}</p>
      <p className="text-meta text-muted-foreground">错误码 {code}</p>
      <Button variant="outline" onClick={onRetry}>
        重试
      </Button>
    </div>
  );
}
