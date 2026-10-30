# CF Gateway

使用 Rust 实现的页面代取服务。自动处理已支持的站点验证、维护会话，返回网站的实际响应。带 Web 控制台、请求历史和阅读书源示例。

服务运行时不启动 Python、Node、浏览器或 nokk。Web 和阅读示例中的 JavaScript 用于界面与 HTML 解析；服务端采用静态协议解析及有限计算模型，不执行网站 JavaScript。

![CF Gateway](assets/web/logo.png)

## 启动

Docker 镜像提供 linux/amd64 和 linux/arm64：

~~~sh
docker run -d --name cf-gateway --restart unless-stopped \
  -p 19686:19686 \
  -e CF_GATEWAY_TOKEN=你的访问令牌 \
  ghcr.io/ifpj/cf-gateway:latest
~~~

打开 http://服务器地址:19686，在右上角填写令牌。留空 CF_GATEWAY_TOKEN 表示不启用访问令牌。也可以使用仓库的 compose.yaml。

首次遇到新的 Cloudflare 初始化程序时需要编译其布局（本机约 1～2.5 秒，低配设备更久）。编译结果会写入 `--layout-cache-dir`（环境变量 `CF_GATEWAY_LAYOUT_CACHE_DIR`，默认系统临时目录下的 `cf-gateway-layout-cache`，填 `none` 关闭），重启后首个请求直接复用，不再重新编译。缓存只含布局结构，按当前程序摘要、版本和辅助函数名校验，并绑定编译它的程序构建；目录不可写时自动关闭，`/v1/status` 的 `runtime.layoutCache` 显示状态。compose.yaml 把 `/tmp` 挂为卷，使缓存在 `read_only` 容器中也能保留。

Web 会自动适配暗色模式。接入示例页使用当前访问地址的域名、端口和 HTTPS，支持手动修改。反向代理应将同一路径前缀下的静态资源和 API 一并转发；公开地址以 Web 中填写的地址为准。

左上角固定显示构建标识，`/v1/status` 返回同样的 `build`（兼容字段 `buildId`）以及 `version`、`commit`、`dirty` 和 `binaryHash`。标识在编译时取自 Git：发布提交上就是版本号（`0.2.14`），之后每个提交自动成为 `0.2.14+3.g79a09af99`（自上个发布以来的提交数和提交），含未提交修改的本地构建再加 `.dirty.<程序哈希>`。同一提交在 amd64 与 arm64 上得到相同标识，不需要手工改动任何数字。版本号只通过 `scripts/dev.ps1 release -Version X.Y.Z` 变更：它更新 Cargo.toml、提交、打 `cf-vX.Y.Z` 标签并推送。CI 只在带标签的提交上发布 `:X.Y.Z` 镜像；每次推送都发布 `:latest`、`:<版本>-<提交数>-g<提交>` 与 `:sha-<提交>`，Cargo.toml 版本与标签不一致时发布失败。修改源码后需重新构建并启动新程序，页面显示的是实际运行版本。

不提供定时健康检测。收到 SIGTERM（`docker stop`）或 Ctrl+C 后停止接受新连接，进行中的请求在各自的 timeoutMs 内完成。容器采用 gcr.io/distroless/static:nonroot，应用全静态链接，只携带可执行文件和许可声明，不含 shell 或 ICU 动态库。语言格式化使用 ICU4X 与内嵌 Unicode 数据。

## 网络设置

默认 **直连、DoH 关闭**，使用系统 DNS。启动参数覆盖同名环境变量，不读写配置文件；Web 仅展示启动默认值，工作台可以为单次请求填写覆盖参数。

| 启动参数 | 环境变量 | 默认值 |
| --- | --- | --- |
| `--network-mode direct` / `proxy` | `CF_GATEWAY_NETWORK_MODE` | `direct` |
| `--proxy URL` | `CF_GATEWAY_PROXY` | 无；proxy 模式必须指定 |
| `--doh true` / `false` | `CF_GATEWAY_DOH` | `false` |
| `--doh-url URL` | `CF_GATEWAY_DOH_URL` | `https://cloudflare-dns.com/dns-query` |

| 连接模式 | DoH 关闭 | DoH 开启 |
| --- | --- | --- |
| 直连 | 系统 DNS，按系统路由连接 | 指定 DoH 解析，按系统路由连接 |
| 代理 | 将目标域名交给指定代理解析和连接 | DoH 查询也经过指定代理，再由代理连接解析得到的 IP |

支持 HTTP、HTTPS、SOCKS5、SOCKS5H 代理。程序不自动读取 HTTP_PROXY 等系统环境代理；直连仍受操作系统路由及 TUN 影响。代理地址本身由系统解析，DoH 地址的引导解析使用当前直连或代理路径，避免递归。显式代理或 DoH 失败不会改用直连或系统 DNS。

~~~sh
./cf-gateway serve
./cf-gateway serve --network-mode proxy --proxy socks5://127.0.0.1:7890
./cf-gateway serve --doh true --doh-url https://cloudflare-dns.com/dns-query
~~~

PowerShell 可设置 `$env:CF_GATEWAY_NETWORK_MODE='proxy'`、`$env:CF_GATEWAY_PROXY='http://127.0.0.1:7890'` 后启动。旧 fixed/doh/hybrid 模式及全局五 IP 映射已移除。

### 单次请求覆盖

`network` 统一容纳 `proxy`、`doh`、`mapping`，所有字段均可省略。省略的选项继承启动配置；`proxy: null`（或空字符串）明确选择直连，代理地址选择代理连接。`doh.enabled` 与 `doh.url` 可分别覆盖。

~~~json
{
  "url": "https://example.com/chapter/1",
  "session": "my-reader",
  "network": {
    "proxy": null,
    "doh": { "enabled": false },
    "mapping": {
      "*": "104.16.184.241:443",
      "example.com:443": "104.17.136.61:443"
    }
  }
}
~~~

`mapping` 最多 32 项，目标必须是 IP:端口，IPv6 使用 `[IP]:端口`。精确的域名:原端口优先，`*` 覆盖本次 API 调用及其跳转、验证子请求中其余所有目标域名和端口；代理服务器、DoH 服务器自身不参与映射。映射命中时跳过目标 DNS，代理模式则让代理连接该 IP:端口；不会自动换其他映射或回退 DNS。映射只改变 TCP 连接目标，不改变 URL 协议、Host、TLS SNI、证书校验及 Cookie 范围，不会将 HTTP 自动转换为 HTTPS。

GET `/fetch`、`/image` 将 `network` 序列化为 JSON 字符串后进行 URL 编码；POST 接受 JSON 对象（也兼容 JSON 字符串）。后续独立的 API 调用不会继承前一次的临时选项。旧请求省略 `network` 正常使用启动默认设置；旧顶层 `proxy`、`connectTo` 保留兼容，同名新选项优先，其中 `network.mapping: {}` 明确清除旧映射。

映射和 DoH 不参与逻辑会话标识；Cookie、验证锁、并发及验证状态维持原规则。连接配置不同的同时请求使用各自的临时传输上下文，不混用映射连接。历史记录保留实际网络设置供复现，代理账号密码不保存，需要重新填写。

## Web 与书源

- 运行概览：累计请求、进行中请求、成功率、进程常驻内存与最近请求。
- 会话列表：完整会话标识、目标来源、验证与使用状态、最近使用时间，支持复制标识和查看该会话请求。一个标识可以对应多个来源，不将条目数量当成设备数；不展示 Cookie 或代理凭证。
- 请求历史：最近 200 条记录，可筛选失败、查看完整错误、导出 JSON、填入工作台重新执行。内存保存，重启清空，不保存网页正文或代理密码。
- 请求工作台：取页、仅分析、导出凭证，支持会话、超时及单次代理、DoH、映射设置。
- 接入示例：一个入口导入全部三份书源，也可下载或复制批量导入链接；服务动态生成，不维护冗余导出文件。

| 示例 | 搜索 | 发现、目录、正文 |
| --- | --- | --- |
| 台湾小说网 | 简体输入，使用阅读内置 s2t 查询 | 支持，输出使用阅读内置 t2s |
| 半夏小说 | 简体输入，使用阅读内置 s2t 查询 | 支持，输出使用阅读内置 t2s |
| 69书吧 | 支持关键词搜索及完整书籍链接检索；服务处理站内嵌入式验证 | 支持 |

示例为当前仓库支持的 JS 书源格式。发现分类与榜单从站点页面读取，翻页跟随页面提供的下一页链接。站点没有分页的栏目不会重复返回第一页。标签名称属于网站内容，入口如“首页推荐”“最新更新”为示例中的导航名称。

## API

默认端口 19686。启用令牌时添加 Authorization: Bearer <令牌>。

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| POST | /v1/fetch | 获取网页，返回 JSON |
| GET | /fetch?url=…&session=… | 返回 HTML 和网站原始 HTTP 状态 |
| GET | /image?url=…&session=… | 原始图片代取，共用验证会话 |
| POST | /v1/solve | 获取验证结果与匹配目标 URL 的 Cookie |
| POST | /v1/analyze | 分析，不提交验证 |
| GET | /v1/status | 运行状态和最近 30 条记录 |
| GET | /v1/history | 最近 200 条请求及复现参数 |
| GET | /sources.json?base=… | 一次导出全部示例书源，JSON 数组包含各自完整 mainJs |
| GET | /source.js?site=twkan&base=… | 保留单份脚本导出；site 还支持 banxia、69shuba |

~~~json
{
  "url": "https://example.com/chapter/1",
  "session": "my-reader",
  "timeoutMs": 60000
}
~~~

network 可选，兼容旧顶层 proxy / connectTo。timeoutMs 范围 1000～120000，包含排队、解析和网络。URL 仅支持 HTTP/HTTPS，不支持用户名密码。响应正文及解压后的正文均限制为 8 MiB。

同站点、会话名、代理复用验证状态，最多 32 个会话；正在使用的会话不会被驱逐。已验证会话最多四路取页，更新验证时独占。页面内容不缓存，Cookie 不写磁盘。只复用 DNS 元数据、连接和精确匹配初始化程序的编译布局；不会复用旧挑战密钥或结果。

同一客户端会话名及代理使用一个 Cookie 容器，由标准 Domain、Path、Secure 规则决定是否发送；主站设置的父域 Cookie 可用于符合范围的图片子域名。不同会话名或代理互不共享。连接、四路读取限制和验证状态仍按完整来源分别维护；某个图片域名的验证不会直接复用主站的“已验证”标志。网络设置不改变此 Cookie 范围。

请求取消仍计入失败，历史保留 `CANCELLED` 和取消时的 `stage`，可区分等待读取名额、等待验证、上游取页及验证取页。不能仅凭取消记录断言上游拒绝，应结合客户端动作复现。

当前已观察到的组件后续提交 HTTP 400，仅允许在原总时限内重新取得一次全新挑战，记录两次尝试及首次错误；不重放被拒绝的提交，不对其他协议错误无限重试。

69书吧的 `/modules/article/search.php?searchkey=…&searchtype=all` 是逻辑搜索地址，服务按该站实际表单使用 POST 提交，保留 GBK 编码。嵌入式验证通过后，提交当前令牌到同站 `/verify.php`，保存站点返回的 Cookie，再重新提交搜索。该适配器仅覆盖69书吧已识别的流程，不执行页面回调，也不开放任意表单提交。网站当前要求两次搜索间隔不少于 5 秒；书源显示上游提示，不将其当成空结果或无限重试。翻页只跟随实际下一页链接。

/v1/fetch 的 ok=true 表示拿到网站实际响应，status 可能是网站的 404。协议或传输失败返回 ok=false、code、error、details；未知路径、方法不符、请求体不是 JSON、缺少 `Content-Type: application/json` 或超过 1 MiB 也返回同样结构（`NOT_FOUND`、`METHOD_NOT_ALLOWED`、`INVALID_REQUEST`、`REQUEST_TOO_LARGE`），调用方只需解析一种错误格式。Web 将处理失败与网站 HTTP 错误分开显示。

书源采用代取模式。跨出口复用 Cookie 不保证可用。阅读的网络导入不能设置请求头，因此只读的 /source.js、/sources.json 与 /image 额外接受 token 查询参数；Web 会在启用令牌时生成相应导入链接。链接包含服务令牌，请按凭证保管。其他 API 仍只接受 Authorization 请求头。令牌不会转发给目标网站。封面统一通过 /image 代取，避免手机直连时遇到 403；该接口保持原始二进制，仅接受可识别签名的 PNG/JPEG/GIF/WebP/AVIF，不转发 SVG 或 HTML。

## 构建

需要 Rust 1.99+。Linux 构建需要 C/C++ 工具链、CMake、Clang/libclang、NASM；Windows 需要 MSVC、CMake、libclang、NASM。无需系统 ICU。Dockerfile 使用 Alpine/musl 构建并执行 release 测试，检查 ELF 无 INTERP / NEEDED 后打包到 distroless/static；提供 linux/amd64、linux/arm64。

~~~sh
cargo fmt --check
cargo test --locked
cargo build --release --locked
cargo run --release -- serve --host 127.0.0.1 --port 19686
~~~

Windows 开发机使用 `scripts/dev.ps1`（`test`、`build`、`run`、`push`），本机构建工具与运行中的服务放在 `.local/`；见 [开发与维护](docs/DEVELOPMENT.md)。

默认绑定本机。容器显式绑定 0.0.0.0。CF_GATEWAY_TIMEZONE 默认 Asia/Shanghai。只有显式传入 --capture-dir 才写诊断文件；发布默认关闭。

## 开发资料

- [开发与维护](docs/DEVELOPMENT.md)
- [编写配套书源](docs/BOOK_SOURCES.md)
- [迁移与验收记录](docs/VALIDATION.md)
- [第三方来源](THIRD_PARTY.md)

能力限于已支持的协议与有限模型；未知任务明确报错。实验性 JSD、69书吧嵌入式 Turnstile 与托管挑战分别验证，不代表所有验证码或所有站点均受支持。网络与站点策略会变化，有限样本的成功和耗时不代表所有站点或后续请求。
