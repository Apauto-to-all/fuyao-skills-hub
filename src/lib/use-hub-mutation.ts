import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "@/components/ui/toast";
import { isGuiError } from "@/lib/ipc";

/**
 * 写操作的统一出口：同一把作用域内串行、成功后让已挂载查询全部失效、
 * 失败文案与锁占用提示只在这里写一次。
 */
export function useHubMutation<TData, TVars>(options: {
  mutationFn: (vars: TVars) => Promise<TData>;
  /** 成功提示；文案要带上结果时传函数 */
  success: string | ((data: TData) => string);
  /** 失败提示的主语，如「解除失败」 */
  failure: string;
  /** 成功后的额外收尾：关弹窗、清选中 */
  onSuccess?: (data: TData) => void;
}) {
  const queryClient = useQueryClient();
  return useMutation({
    scope: { id: "hub-write" },
    mutationFn: options.mutationFn,
    onSuccess: (data) => {
      toast.add({
        title:
          typeof options.success === "function"
            ? options.success(data)
            : options.success,
        type: "success",
      });
      // 粗粒度失效：数据全在本地，重拉代价可忽略；未挂载的查询失效是空操作
      void queryClient.invalidateQueries();
      options.onSuccess?.(data);
    },
    onError: (error) => {
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
