# 版本检查的发布源取 npm registry

## 背景

版本检查要回答「有没有比当前更新的版本」。候选发布源有两个：GitHub Releases API，或 npm registry 的 dist-tags。gh、terraform 等多数 CLI 查 GitHub，看起来是行业惯例。

但惯例的成因是它们的安装渠道：`brew upgrade gh` 装的就是 GitHub 的产物，查 GitHub 才对得上。fsh 的安装渠道是 npm——升级提示里让用户敲的是 `npm i -g fuyao-skills-hub@latest`，`@latest` 解析到哪个版本，由 registry 的 `latest` dist-tag 决定，不由 GitHub Release 决定。

两者会不同步：Release 打了 tag、`npm publish` 那步失败时（CI 里已有「查 npm 是否已发布」的分支在防这种情况），GitHub 说新版可用，用户照着提示装回来还是旧版——提示就成了假话。

## 备选

- GitHub Releases API：与多数 CLI 一致，`tag_name` 现成。但响应 4KB、未认证限流 60 次/小时/**IP**（公司 NAT、CI 池共享出口时不够用）、必须带 `User-Agent` 否则 403、`v` 前缀要剥。放弃。
- GitHub 的 `/releases/latest` 重定向，只读 `Location` 头：无速率限制、响应体 0 字节。但依赖 GitHub 不改这个跳转，属未文档化的约定，不是契约。放弃。
- npm registry 的 `/-/package/<名>/dist-tags`：20 字节，无限流，无需 `User-Agent`，返回的正是那条升级命令会装到的版本。采用。

## 决策

版本检查查 `https://registry.npmjs.org/-/package/fuyao-skills-hub/dist-tags`，取配置里 `channel` 指定的那个 tag（缺省 `latest`）。

发布源写进配置 `hub.toml` 的 `[update].registry`，便于换镜像。

## 理由

- **查的必须是提示里那条命令的源头。** 提示说「升级到 0.1.3」，用户敲完命令就该装到 0.1.3；只有 registry 能保证这一点。
- dist-tags 端点顺带白送了 `channel`（`latest` / `next` / `beta`）这个配置项：换 tag 名即可，不用改代码。
- 无速率限制，一天一次的被动检查不该有隐藏前提。

将来若改为 GitHub Releases 分发，换 URL 并剥 `v` 前缀即可，改动限于一个函数。
