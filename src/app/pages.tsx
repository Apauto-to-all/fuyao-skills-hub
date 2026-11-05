import {
  Activity,
  Bot,
  FolderGit2,
  Library,
  type LucideIcon,
} from "lucide-react";
import type { ComponentType } from "react";
import { AgentsPage } from "./agents";
import { HealthPage } from "./health";
import { LibraryPage } from "./library";
import { ProjectsPage } from "./projects";

/** 页面标识：导航、命令面板与后续深链都只认这几个值 */
export type PageKey = "library" | "projects" | "agents" | "health";

export type PageDef = {
  key: PageKey;
  label: string;
  description: string;
  icon: LucideIcon;
  // 库页可接收「命令面板跳技能」的键与触发序号；其余页忽略该 prop
  component: ComponentType<{
    focusKey?: { key: string; seq: number } | null;
  }>;
};

/** 四页注册表：加页面只动这里，导航、顶栏与命令面板自动跟上 */
export const PAGES: PageDef[] = [
  {
    key: "library",
    label: "库",
    description: "技能库",
    icon: Library,
    component: LibraryPage,
  },
  {
    key: "projects",
    label: "项目",
    description: "项目占用",
    icon: FolderGit2,
    component: ProjectsPage,
  },
  {
    key: "agents",
    label: "Agent",
    description: "Agent 落点",
    icon: Bot,
    component: AgentsPage,
  },
  {
    key: "health",
    label: "健康",
    description: "检查与回收",
    icon: Activity,
    component: HealthPage,
  },
];
