import { Button } from "@/components/ui/button";
import { isGuiError } from "@/lib/ipc";

type Props = {
  hubRoot: string;
  pending: boolean;
  error: unknown;
  onInit: () => void;
  onOpenDir: () => void;
};

/** 未初始化引导：四页共用同一条出口，初始化成功后就地进入正常页 */
export function InitGuide({
  hubRoot,
  pending,
  error,
  onInit,
  onOpenDir,
}: Props) {
  return (
    <div className="mx-auto flex max-w-lg flex-col items-center gap-3 px-6 py-24 text-center">
      <h2 className="text-lg font-semibold">还没有数据目录</h2>
      <p className="text-xs text-muted-foreground">
        Fuyao Skills Hub 需要在应用目录下建立自己的数据目录，才能开始管理技能。
      </p>
      <code className="mt-1 w-full break-all rounded-lg border border-border bg-muted/40 px-3 py-2 text-left font-mono text-xs">
        {hubRoot}
      </code>
      <div className="mt-2 flex gap-2">
        <Button onClick={onInit} disabled={pending}>
          {pending ? "初始化中…" : "初始化"}
        </Button>
        <Button variant="outline" onClick={onOpenDir}>
          打开数据目录
        </Button>
      </div>
      {error ? (
        <p role="alert" className="text-xs text-destructive">
          {isGuiError(error) ? error.message : String(error)}
        </p>
      ) : null}
    </div>
  );
}
