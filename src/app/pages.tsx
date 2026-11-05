import {
  Activity,
  Bot,
  FolderGit2,
  Library,
  type LucideIcon,
} from "lucide-react";
import type { ComponentType } from "react";
import { LibraryPage } from "./library";

/** 页面标识：导航、命令面板与后续深链都只认这几个值 */
export type PageKey = "library" | "projects" | "agents" | "health";

export type PageDef = {
  key: PageKey;
  label: string;
  description: string;
  icon: LucideIcon;
  component: ComponentType;
};

/** 占位页：后续票各自换成真页面，壳与注册表不随之改动 */
function Placeholder({ title, hint }: { title: string; hint: string }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-1.5 px-6 text-center">
      <p className="text-ui font-medium">{title}页</p>
      <p className="text-ui-sm text-muted-foreground">{hint}</p>
    </div>
  );
}

function ProjectsPage() {
  return <Placeholder title="项目" hint="项目列表由「使用」票实现" />;
}

function AgentsPage() {
  return <Placeholder title="Agent" hint="Agent 列表由「使用」票实现" />;
}

function HealthPage() {
  return (
    <Placeholder title="健康" hint="检查与回收由「健康与全局收口」票实现" />
  );
}

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
