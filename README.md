# 番茄小说 Rust 中转

Rust 负责官方接口请求、签名、匿名设备与正文密钥、解密解压、目录及元数据转换。阅读书源只传递小型请求并处理网页登录和阅读事件。Web 控制台、书评页面及书源模板全部嵌入程序，部署只需一个服务或 Docker 镜像，用户从 Web 页面导入生成的 JSON 书源即可。

## Docker 部署

镜像：`ghcr.io/ifpj/fanqie-relay:latest`，支持 **linux/amd64、linux/arm64**。同时提供当前版本标签及 `sha-完整提交SHA` 标签，后者适合固定部署版本。

在当前目录执行：

```sh
cp .env.example .env
# 可直接使用默认配置；需要换外部端口时修改 FANQIE_RELAY_PORT
# 需要访问令牌时填写 FANQIE_RELAY_TOKEN；留空即允许直接访问。
docker compose pull
docker compose up -d
docker compose logs -f
```

在手机上打开 `http://主机地址:外部端口/`，导入链接、二维码和生成的书源自动使用当前访问的域名、IP、端口及协议。例如映射 `122:19670` 后，打开 `http://your-server.lan:122/` 即会生成该地址的书源，无需配置固定 IP。外部端口可通过 `FANQIE_RELAY_PORT` 调整，服务内部仍监听 19670。手机上的 `127.0.0.1` 指向手机自身。

`FANQIE_RELAY_PUBLIC_URL` 默认留空；仅需要固定导出地址时填写，也可在 Web 手动覆盖。生成地址的优先级为 `?base=` → 配置的固定地址 → 请求地址；反向代理支持标准 `Forwarded` 或 `X-Forwarded-Host` / `X-Forwarded-Proto`，代理应覆盖这些头并传递外部域名（含端口）及协议。当前支持部署在域名根路径。

运行镜像使用 `gcr.io/distroless/static:nonroot`，Rust 在 musl 环境静态构建并检查无动态链接依赖；不包含 Shell、包管理器或 curl，未启用容器定时健康检测。Compose 使用非 root 用户、只读文件系统，不需要挂载书源、数据库或配置目录。

也可在服务根目录自行构建：

```sh
docker build -f Dockerfile -t fanqie-relay:local .
# 在支持两个平台的 Buildx builder 中构建并上传：
docker buildx build --platform linux/amd64,linux/arm64 \
  -f Dockerfile -t your-registry/fanqie-relay:latest --push .
```

GitHub Actions 在 `fanqie` 分支更新相关文件时自动运行，也可在 Actions 重新运行已有构建。AMD64 和 ARM64 分别在对应的原生 runner 构建；每个平台运行 Rust 单元测试、书源回调测试，以及实际容器的匿名/令牌访问、嵌入页面与书源导出、精确 ID、完整元数据、凭证隐藏、gzip 和正常停止检查。两者通过后合并镜像 manifest 并发布至 GHCR，构建缓存按架构隔离。流程见 [fanqie-relay-docker.yml](.github/workflows/fanqie-relay-docker.yml)，平台构建方式参考 [Docker 官方说明](https://docs.docker.com/build/ci/github-actions/multi-platform/)。本仓库镜像公开，可免登录拉取。

## 书源导入与登录

打开 Web 控制台的“接入书源”，在阅读的“书源管理 → 菜单 → 扫描二维码”扫描书源二维码，可直接导入；也可使用一键导入、网络导入链接，或下载生成的 JSON。`source/` 里的 JS 和 JSON 是编译资源，运行时无需额外文件；请使用生成的 `/source.json`，其中包含阅读事件、批量正文和章节同步配置。

登录在阅读内置浏览器的番茄官方网页完成，返回时点勾号。登录后支持分组书架、历史记录及章节双向同步。真实切换章节时上传；打开书籍、退出或重新进入时读取云端章节。当前已打开的阅读页需重新进入才能应用云端位置，无需开启阅读 Web 服务。只同步章节，不同步页内位置。

上传接口成功、立即回读仍为旧值时，返回 `accepted` / `verification.status=pending`；仅在章节 ID 与时间戳确认后返回 `uploaded` / `confirmed`。已受理的提交保留确认凭据，不误报上传失败、不因旧回读回跳或重复上传。较新的云端位置优先，冲突锁只协调同一服务实例。

书架增删联动、原生段评默认关闭，可在书源登录界面的设置中开启。阅读定制按钮提供书评、章节讨论、段评和回复，均只读，不发表评论或点赞。

## 功能与运行行为

- 搜索、ID/分享链接搜索、详情、完整目录、单章与批量正文；每批最多 30 章，同设备正文串行，前一批完成后立即继续，没有人为批次间隔。
- 首页、猜你喜欢使用频道接口的 `next_offset` 和会话连续翻页；`bottom_unlimited=true` 时持续加载，不将顶层 `has_more=false` 误判为结束。高分栏目换批追加；分类、标签、字数/完结/更新筛选及男女频道。榜单范围、名称、编号和栏目 ID 每次刷新从官方完整榜单导航读取，沿用官方顺序；榜单分页沿用官方 `next_offset`、会话和榜单版本。作者、话题和视频等非书籍榜单不作为阅读书籍列表，完整导航原文仍在 Web 请求详情中保留。有限榜单、书架和历史在实际数据结束时停止。
- 完整 API 元数据保留在书籍/章节的 `variable.fanqie`；映射字数、更新时间、分卷、VIP、阅读上下文，保留插图和脚注。长整数 ID 不损失精度。
- “全部小说”是服务提供的不限定题材入口，显示所选男生／女生频道的小说列表，沿用筛选设置；通过官方新版合并分类页加载，不代表一次列出或下载整个书库。
- Web 提供请求/连接总数、成功率、耗时、阶段耗时、内存占用、近期记录、官方 JSON 原文、字段树与下载；自动跟随系统浅色/暗色偏好。
- 不缓存书籍 API 响应。复用连接、设备、密钥和当前分页游标/余量；刷新第一页重新请求。诊断详情按 32 MiB 负载预算保留，仅用于查看；最多 200 条请求、40 条连接，重启清空。
- 全线上游使用固定阿里 DoH `https://dns.alidns.com/dns-query`，IPv6 优先并保留 IPv4；仅按 DNS TTL 复用解析结果，不改变系统网络设置。Docker 主机无需 IPv6 也可使用 IPv4。

## 配置

| 环境变量 | 默认值 / 用途 |
|---|---|
| `FANQIE_RELAY_LISTEN` | 原生程序 `127.0.0.1:19670`，Docker `0.0.0.0:19670` |
| `FANQIE_RELAY_PUBLIC_URL` | 留空自动使用请求地址；可填写 HTTP(S) 固定导出地址 |
| `FANQIE_RELAY_TOKEN` | 空值表示免令牌；否则使用 Bearer 验证 |
| `FANQIE_RELAY_CONCURRENCY` | 默认 8，可设置 1～64；同设备正文并发始终为 1 |

启用令牌后，在 Web 右上角填入令牌，使用“下载 JSON 书源”从文件导入（此时隐藏扫码和一键导入）；导出的文件含令牌，网络导入链接不携带令牌。账号凭证仍由阅读提供，服务不持久保存账号会话。诊断记录隐藏 cookie、token、密码、签名和解密密钥。填写后的 `.env` 已忽略，请勿提交。

## 本地构建

需 Rust 1.97.1 或更高版本。Linux 在服务目录执行：

```sh
cargo test --locked
cargo build --release --locked
FANQIE_RELAY_LISTEN=0.0.0.0:19670 ./target/release/fanqie-relay
```

Windows 需 Rust 及 Visual Studio C++ Build Tools，在服务根目录执行：

```powershell
./run-windows.ps1 -Build -Test
./run-windows.ps1 -Stop
```

已有进程运行时，先停止再重新构建。脚本后台运行，日志和 PID 保存在忽略的 `runtime/`。不设置开机自启、不修改防火墙。`-Development` 可用于从 `web/` 读取页面；普通构建全部内嵌。书源回调测试：

```sh
node tests/check-source-sync.cjs
node tests/check-source-discovery.cjs
```

## HTTP 接口

| 方法 / 路径 | 用途 |
|---|---|
| `GET /health` | 版本和健康检查，免令牌 |
| `POST /v1/call` | 搜索、详情、目录、正文、发现、书架、评论、进度及受限原始 API |
| `GET /admin/status` | 请求、连接、耗时与内存统计 |
| `GET /admin/requests/{id}` | 完整诊断数据 |
| `GET /admin/network` | DoH 状态，只读 |
| `GET /admin/catalog` | 官方接口与字段说明 |
| `GET /source.json?base=...` | 生成包含内嵌 mainJs 的 JSON 书源；`download=1` 下载 |
| `GET /source.js?base=...` | 生成 JS 书源；单独使用不保留完整阅读事件配置 |
| `GET /community?bookId=...&chapterId=...` | 书评与讨论 |
| `GET /qr.svg?base=...` | 阅读 App 书源扫码导入二维码；启用令牌时不可用 |

书源请求和工作台使用相同接口。参数、书籍/章节 ID 使用字符串；账号由请求的 `account` 携带 `sessionCookie`、`uid`、可选 `ttToken`/`sessionSign`。`raw` 操作只接受接口目录列出的路径。

## 许可证与协议来源

遵循仓库 [GPL-3.0 许可证](LICENSE)。签名与 TTEncrypt 协议参考 [naiyQAQ/fanqie-assistant](https://github.com/naiyQAQ/fanqie-assistant)。接口对照版本为番茄 Android `7.3.9.32`；运行服务无需安装原 APK。签名测试向量使用合成设备 ID、固定时间和随机数，不含真实账号信息。
