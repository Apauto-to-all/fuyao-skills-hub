#!/usr/bin/env node
// fsh 的 npm 转发 shim：定位包内 exe，转发参数与退出码。
// 平台多起来时把这里换成按 process.platform 解析平台子包的查找表。
const { spawnSync } = require("child_process");
const { join } = require("path");

const exe = join(__dirname, "..", "bin", "fsh.exe");

const result = spawnSync(exe, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  console.error(`fsh: 无法启动 ${exe}: ${result.error.message}`);
  process.exit(1);
}
process.exit(result.status ?? 1);
