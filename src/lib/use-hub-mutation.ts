import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "@/components/ui/toast";
import { WRITE_FILLED_QUERY_KEYS } from "@/lib/hub-queries";
import { isGuiError } from "@/lib/ipc";

/**
 * 写操作的统一出口：同一把作用域内串行、成功后让已挂载查询失效、
 * 失败文案与锁占用提示只在这里写一次。初始化与打开数据目录也走这里，
 * 错误文案与失效策略因此只有一份。
 */
export function useHubMutation<TData, TVars>(options: {
  mutationFn: (vars: TVars) => Promise<TData>;
  /** 成功提示；文案要带上结果时传函数。缺省不弹（结果由页面自己展示） */
  success?: string | ((data: TData) => string);
  /** 失败提示的主语，如「解除失败」；缺省不弹（错误由页面就地展示） */
  failure?: string;
  /** 成功后是否让已挂载查询失效；false 表示这次写不改读侧数据 */
  invalidate?: boolean;
  /** 串行作用域；缺省与其它 hub 写操作串行，传 null 则不参与串行 */
  scope?: string | null;
  /** 成功后的额外收尾：关弹窗、清选中 */
  onSuccess?: (data: TData) => void;
}) {
  const queryClient = useQueryClient();
  return useMutation({
    scope:
      options.scope === null ? undefined : { id: options.scope ?? "hub-write" },
    mutationFn: options.mutationFn,
    onSuccess: (data) => {
      if (options.success !== undefined) {
        toast.add({
          title:
            typeof options.success === "function"
              ? options.success(data)
              : options.success,
          type: "success",
        });
      }
      // 粗粒度失效：数据全在本地，重拉代价可忽略；未挂载的查询失效是空操作。
      // 由写操作自己填结果的槽位跳过，否则执行结果会被重拉打回写之前的读数
      if (options.invalidate !== false) {
        void queryClient.invalidateQueries({
          predicate: (query) =>
            !WRITE_FILLED_QUERY_KEYS.includes(String(query.queryKey[0])),
        });
      }
      options.onSuccess?.(data);
    },
    onError: (error) => {
      if (options.failure === undefined) return;
      toast.add({
        // 锁被另一个进程占住是可预期的竞争，给一句能照做的提示
        title:
          isGuiError(error) && error.code === "lock_held"
            ? "另一个 fsh 进程正在写，稍后重试"
            : options.failure,
        description: isGuiError(error) ? error.message : String(error),
        type: "error",
      });
    },
  });
}
