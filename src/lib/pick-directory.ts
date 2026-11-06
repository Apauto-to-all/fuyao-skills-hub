import { open } from "@tauri-apps/plugin-dialog";
import { toast } from "@/components/ui/toast";
import { isGuiError } from "@/lib/gui-error";

/**
 * 目录选择的统一出口：失败提示只在这里写一次；
 * 返回 null 表示用户取消或选择器打不开。
 */
export async function pickDirectory(title: string): Promise<string | null> {
  try {
    const picked = await open({ directory: true, multiple: false, title });
    return typeof picked === "string" ? picked : null;
  } catch (error) {
    toast.add({
      title: "无法打开目录选择器",
      description: isGuiError(error) ? error.message : String(error),
      type: "error",
    });
    return null;
  }
}
