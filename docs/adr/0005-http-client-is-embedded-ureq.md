# 版本检查内嵌 ureq，不起外部 HTTP 进程

## 背景

版本检查需要发一次 HTTPS 请求。fsh 目前没有任何 HTTP 能力，`git2` 虽已静态链接但只用于 git 操作。三种做法：调系统 `curl`、用 `windows-sys` 的 WinHttp（该 crate 已因 `junction` 进入依赖树）、或引入一个 Rust HTTP 客户端。

curl 在 Windows 10 1803+ 与 macOS 上稳定存在，但在 Linux 上不可靠：Alpine 只有 BusyBox 的 `wget`，debian-slim 与 distroless 默认不带 curl。项目要转多平台，而 Linux 恰是 CI 与容器运行的地方——一个「机器上没有 curl 就静默失效」的检查功能，比没有更糟，因为用户会以为它在工作。

## 备选

- 系统 `curl` 子进程：零新依赖、零 TLS 体积。但 Linux 覆盖不可靠，且要靠 `--max-time` 兜超时。放弃。
- `windows-sys` 的 WinHttp：crate 已在依赖树里，不下载新东西。但需要约 60 行 `unsafe` FFI，而 `crates/` 目前零 `unsafe`，且只覆盖 Windows，转多平台时要再写一套。放弃。
- `reqwest` / `native-tls` 路线：Linux 上牵出 OpenSSL，交叉编译立刻变成负担。放弃。
- `ureq` 3 + 默认 rustls：纯 Rust、`#![forbid(unsafe_code)]`、TLS 全静态编进二进制、不碰系统 OpenSSL，交叉编译各平台不需要目标机装 `libssl-dev`。采用。

## 决策

引入 `ureq`，关闭默认的 `gzip`（响应只有 20 字节，不需要解压），只留 `rustls`；证书用默认的 webpki-roots（Mozilla 根证书静态编进二进制）。查询函数用同步阻塞调用，`timeout_global` 3 秒。

## 理由

- 版本检查这段逻辑三平台完全同形，现在写成跨平台的，将来加 Linux / macOS 目标时一行不用改。
- 纯 Rust 与零 `unsafe` 的现状一致，不引入 C 工具链依赖。
- 代价是二进制体积预计增 2–3MB（ring + rustls + 根证书）。对一个被动检查功能，这是保险费，不是浪费。

根证书随版本静态固化，不随系统更新：装三年不升级可能遇到过期根证书。真报错时再切 `platform-verifier` 读系统信任库，改动限于 TLS 配置一处。
