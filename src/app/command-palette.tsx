import { useQuery } from "@tanstack/react-query";
import { FolderOpen, RotateCw } from "lucide-react";
import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
} from "@/components/ui/command";
import { api } from "@/lib/ipc";
import { PAGES, type PageKey } from "./pages";

type Props = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onNavigate: (key: PageKey) => void;
  onOpenSkill: (key: string) => void;
  onOpenDir: () => void;
  onRefresh: () => void;
};

/** 命令面板：只管「去某处」与两项只读动作，写操作不进面板 */
export function CommandPalette({
  open,
  onOpenChange,
  onNavigate,
  onOpenSkill,
  onOpenDir,
  onRefresh,
}: Props) {
  // 技能清单只在面板打开时取，关着不占请求
  const skills = useQuery({
    queryKey: ["skills"],
    queryFn: () => api.listSkills(),
    enabled: open,
  });

  const run = (action: () => void) => {
    action();
    onOpenChange(false);
  };

  return (
    <CommandDialog
      open={open}
      onOpenChange={onOpenChange}
      title="命令面板"
      description="跳转页面、打开技能或执行只读动作"
    >
      <Command>
        <CommandInput placeholder="搜索页面或技能…" />
        <CommandList>
          <CommandEmpty>没有匹配项</CommandEmpty>
          <CommandGroup heading="页面">
            {PAGES.map(({ key, label, icon: Icon }) => (
              <CommandItem
                key={key}
                value={`${label} ${key}`}
                onSelect={() => run(() => onNavigate(key))}
              >
                <Icon className="text-muted-foreground" strokeWidth={1.75} />
                {label}
              </CommandItem>
            ))}
          </CommandGroup>
          {skills.isError ? (
            <>
              <CommandSeparator />
              <CommandGroup heading="技能">
                <CommandItem value="技能清单读取失败" disabled>
                  技能清单读取失败，重开面板重试
                </CommandItem>
              </CommandGroup>
            </>
          ) : null}
          {skills.data && skills.data.length > 0 ? (
            <>
              <CommandSeparator />
              <CommandGroup heading="技能">
                {skills.data.map((skill) => (
                  <CommandItem
                    key={skill.key}
                    value={`${skill.key} ${skill.description ?? ""}`}
                    onSelect={() => run(() => onOpenSkill(skill.key))}
                  >
                    <span className="truncate font-mono text-meta">
                      {skill.key}
                    </span>
                    {skill.description ? (
                      <span className="truncate text-meta text-muted-foreground">
                        {skill.description}
                      </span>
                    ) : null}
                  </CommandItem>
                ))}
              </CommandGroup>
            </>
          ) : null}
          <CommandSeparator />
          <CommandGroup heading="动作">
            <CommandItem value="打开数据目录" onSelect={() => run(onOpenDir)}>
              <FolderOpen
                className="text-muted-foreground"
                strokeWidth={1.75}
              />
              打开数据目录
            </CommandItem>
            <CommandItem value="刷新当前页" onSelect={() => run(onRefresh)}>
              <RotateCw className="text-muted-foreground" strokeWidth={1.75} />
              刷新当前页
            </CommandItem>
          </CommandGroup>
        </CommandList>
      </Command>
    </CommandDialog>
  );
}
