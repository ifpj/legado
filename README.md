# CF Gateway

使用 Rust 实现的页面代取服务。自动处理已支持的站点验证、维护会话，返回网站的实际响应。带 Web 控制台、请求历史和阅读书源示例。

服务运行时不启动 Python、Node、浏览器或 nokk。Web 和阅读示例中的 JavaScript 用于界面与 HTML 解析；服务端采用静态协议解析及有限计算模型，不执行网站 JavaScript。

![CF Gateway](assets/logo.png)

## 启动

Docker 镜像提供 linux/amd64 和 linux/arm64：

~~~sh
docker run -d --name cf-gateway --restart unless-stopped \
  -p 19686:19686 \
  -e CF_GATEWAY_TOKEN=你的访问令牌 \
  ghcr.io/ifpj/cf-gateway:latest
~~~

打开 http://服务器地址:19686，在右上角填写令牌。留空 CF_GATEWAY_TOKEN 表示不启用访问令牌。也可以使用仓库的 compose.yaml。

Web 会自动适配暗色模式。接入示例页使用当前访问地址的域名、端口和 HTTPS，支持手动修改。反向代理应将同一路径前缀下的静态资源和 API 一并转发；公开地址以 Web 中填写的地址为准。

不提供定时健康检测。容器采用 distroless/cc-debian12:nonroot，只携带可执行文件、ICU 运行库和许可声明，不含 shell。

## Web 与书源

- 运行概览：累计请求、进行中请求、成功率、进程常驻内存与最近请求。
- 请求历史：最近 200 条记录，可筛选失败、查看完整错误、导出 JSON、填入工作台重新执行。内存保存，重启清空，不保存网页正文或代理密码。
- 请求工作台：取页、仅分析、导出凭证，支持会话、超时及可选代理。
- 接入示例：服务动态生成书源，不需要另行维护导出文件。

| 示例 | 搜索 | 发现、目录、正文 |
| --- | --- | --- |
| 台湾小说网 | 简体输入，使用阅读内置 s2t 查询 | 支持，输出使用阅读内置 t2s |
| 半夏小说 | 简体输入，使用阅读内置 s2t 查询 | 支持，输出使用阅读内置 t2s |
| 69书吧 | **仅支持完整书籍链接检索**；站内关键词搜索另需独立嵌入式验证，当前未实现 | 支持 |

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
| GET | /source.js?site=twkan&base=… | 动态导出书源；site 还支持 banxia、69shuba |

~~~json
{
  "url": "https://example.com/chapter/1",
  "session": "my-reader",
  "timeoutMs": 60000
}
~~~

proxy 可选，支持 HTTP/HTTPS/SOCKS。timeoutMs 范围 1000～120000，包含排队、解析和网络。URL 仅支持 HTTP/HTTPS，不支持用户名密码。响应正文及解压后的正文均限制为 8 MiB。

同站点、会话名、代理复用验证状态，最多 32 个会话；正在使用的会话不会被驱逐。已验证会话最多四路取页，更新验证时独占。页面内容不缓存，Cookie 不写磁盘。只复用 DNS 元数据、连接和精确匹配初始化程序的编译布局；不会复用旧挑战密钥或结果。

/v1/fetch 的 ok=true 表示拿到网站实际响应，status 可能是网站的 404。协议或传输失败返回 ok=false、code、error、details。Web 将处理失败与网站 HTTP 错误分开显示。

书源采用代取模式。跨出口复用 Cookie 不保证可用。阅读的网络导入不能设置请求头，因此只读的 /source.js 与 /image 额外接受 token 查询参数；Web 会在启用令牌时生成相应导入链接。链接包含服务令牌，请按凭证保管。其他 API 仍只接受 Authorization 请求头。令牌不会转发给目标网站。封面统一通过 /image 代取，避免手机直连时遇到 403；该接口保持原始二进制，仅接受可识别签名的 PNG/JPEG/GIF/WebP/AVIF，不转发 SVG 或 HTML。

## 构建

需要 Rust 1.99+。Linux 需要 C/C++ 工具链、CMake、Clang/libclang、NASM、ICU；Windows 需要 MSVC、CMake、libclang、NASM，ICU 使用系统库。Dockerfile 提供可复现的 Linux 构建环境。

~~~sh
cargo fmt --check
cargo test --locked
cargo build --release --locked
cargo run --release -- serve --host 127.0.0.1 --port 19686
~~~

默认绑定本机。容器显式绑定 0.0.0.0。CF_GATEWAY_TIMEZONE 默认 Asia/Shanghai。只有显式传入 --capture-dir 才写诊断文件；发布默认关闭。

## 开发资料

- [开发与维护](docs/DEVELOPMENT.md)
- [编写配套书源](docs/BOOK_SOURCES.md)
- [迁移与验收记录](docs/VALIDATION.md)
- [第三方来源](THIRD_PARTY.md)

能力限于已支持的协议与有限模型；未知任务明确报错。实验性 JSD 路径、独立嵌入式验证码与已验证的托管挑战路径不能混为同一项验收。网络与站点策略会变化，有限样本的成功和耗时不代表所有站点或后续请求。
