# 开发与维护

本分支为独立 Rust 工程，根目录就是 Cargo 工程。原 Python 参考实现、调试抓包、真机脚本、构建目录和个人运行配置不进入本分支。

## 模块

| 模块 | 职责 |
| --- | --- |
| web / assets | Axum 路由、静态 Web、访问令牌与请求历史界面 |
| service | 会话隔离、32 个会话上限、四路读取、独占验证、超时和取消统计 |
| network | 原生 HTTP/TLS、重定向、Cookie、系统 DNS / 可配置 DoH、显式代理与请求级 IP 映射 |
| engine | 从初始请求到独立复读目标页面的完整协议流程 |
| constants / compiler / compiler_profiles / data | 静态语法、常量恢复、有限模型和数据校验 |
| catalog / models / model_geometry / surface / locale / semantics / declared | 按当轮输入计算已识别任务 |
| wire / completion / http_tasks | 编解码、后续阶段判定、受约束的 HTTP 子任务 |
| sources / assets/*-parser.js | 动态书源导出及各站点 HTML 解析 |
| embedded | 已知站点的嵌入式验证声明、搜索表单与验证 Cookie 约束 |

profiles 是显式声明、公式和不可变协议布局，不是已计算的浏览器指纹或可复用验证凭证。当前软件模型不声称测量了真实浏览器或发生了人工交互。

Cookie 上下文按客户端会话名和代理隔离，使用弱引用索引；没有存活来源会话时不会额外保留 Cookie。来源连接、验证锁、读取限额和验证代数独立，禁止把父域 Cookie 的共享误写成多个来源共用“已验证”标志。网络覆盖参数不能清空共享容器内其他域名的凭证。

`NetworkConfig` 的启动默认值是 direct / 系统 DNS（DoH 关闭）。四个启动选项与环境变量提供连接模式、显式代理、DoH 开关及服务器地址；参数覆盖环境，Web 只读，不持久化配置。`network.proxy/doh/mapping` 是单次请求覆盖，缺省继承启动值；保留顶层 proxy/connectTo 兼容，新字段逐项优先。取消全局五 IP、CF 网段分类及线路回退，不因失败静默改变用户指定的路径。

network/config 校验 namespace、精确 authority 与 `*`；network/dns_wire 实现有界 RFC 8484 二进制 DNS，resolver 维护最多 256 项 TTL DNS 缓存及并发查询合并，失败不缓存。DoH 端点由正常系统/代理路径引导，不使用硬编码 IP。代理模式的 DoH 查询也经同一代理，不以本地解析代替远端解析。

network/transport 使用原生 loopback SOCKS 隧道统一 HTTP CONNECT / HTTPS CONNECT / SOCKS5 的连接目标；原因是只覆写解析器不能改变 URL 的显式端口，也不能要求 HTTP 代理按指定 IP 建隧道。原 URL 仍交给 HTTP/TLS 层，Host/SNI/证书校验保持原域名；HTTPS 代理自身使用受信任根证书验证。映射优先于 DNS，精确匹配优先于 `*`，代理及 DoH 基础连接不经过目标映射。

请求 task-local 上下文携带临时 DNS/映射与传输，网络子任务通过 inherit_context 显式继承；请求结束释放，不改变来源/客户端/代理的会话索引或客户端/代理的 Cookie 索引。不同配置不能复用同一映射连接；未覆盖默认设置的请求复用原 Session 连接池。离线测试以独立 DNS、HTTP/SOCKS 代理、跳转和 TLS SNI fixture 验证实际路由，不将策略只做成状态标签。

`CANCELLED` 保留在失败统计中，新增 stage 仅用于定位取消发生于排队、上游读取还是验证。必须在客户端动作与服务端记录之间建立对应，并测试返回该列表项后能否重新加载；不要通过更改统计分母或无限重试来掩盖加载问题。

Wasmtime 只用于匹配已审核摘要、无外部导入且带内存/指令/调用限制的有限算术模块，不是任意网站程序运行入口。Tree-sitter 用于读取语法，不执行 JavaScript。

## 修改约束

1. 先定位失败阶段。HTTP 200、Cookie 颁发、工作量计算完成都不等于取得目标网页。
2. 不能使用旧 Cookie、录制答案、固定任务结果来隐藏协议失败。未知格式应保留明确错误。
3. 不添加网页响应缓存。DNS 元数据和精确程序布局可复用，但不得携带旧密钥、任务输入或答案。
4. 同一总时限覆盖排队、解析、验证与重定向，不能用无界重试伪造稳定性。
5. 新的辅助函数必须进入实际运行路径后再用于对照，避免只在测试中“实现”兼容。
6. 不改阅读应用来掩盖示例解析问题。示例站点应在指定的真实阅读环境中验收。
7. 抓包默认关闭，提交前检查 fixture 和文档，不提交个人令牌、代理密码、账号会话、响应抓包和绝对开发路径。

## 检查

```sh
cargo fmt --check
cargo test --locked
for file in assets/*.js tests/*.cjs; do node --check "$file"; done
node tests/source-contracts.cjs
node tests/twkan-cleanup.cjs
node tests/content-cleanup.cjs
docker build -t cf-gateway:test .
```

Rust 测试不访问真实网站。tests/smoke-container.cjs 需要先启动容器，CI 使用宿主网络让容器访问临时本地上游；脚本不依赖外网和账号。CI 分别在原生 amd64、arm64 runner 构建、运行测试、验证实际容器，然后合并镜像清单。

本机手动测耗时应采用 release 构建，分别记录冷进程/新会话/复用会话。保留全部失败与慢样本；记录墙钟总耗时和服务内部耗时，不能将本地计算时间冒充端到端时间。真实网站验收必须检查目标页面的业务标记；例如 Linux.do 测试地址的真实网站页面可能是 404，不能只以 HTTP 200 判断。

## 错误复现

Web“请求历史”可筛选处理失败，查看 code、error、details、排队和总耗时。点击“填入工作台”后手动执行；原会话可能已经变化，可以同时比较原会话和新会话。历史只保留 200 条和复现所需参数，不保存页面正文、令牌、Cookie 或代理账号密码。含账号代理需要重新输入。

--capture-dir 仅用于显式本地诊断；输出可能包含短期挑战状态，不能直接上传。用于报告的 URL 应移除临时验证参数。不要为了调试绕过 TLS 证书校验或修改系统代理配置。

## 平台

语言区域模块使用锁定版本的 ICU4X 2.3，数据内嵌于二进制；不再动态加载 Windows ICU DLL 或 Linux ICU .so。月份/时区、语言名称、析取列表和紧凑数字均使用对应组件，语言名称采用带本地化组合模板的接口。遵循 CF_GATEWAY_LOCALE，缺省读取系统语言，C/POSIX 或无语言设置回退 en-US；时区仍由 CF_GATEWAY_TIMEZONE 指定。

Docker 使用 Rust 1.99 / Alpine 3.24 的 musl 目标，构建期的 C/C++ 工具链只用于 TLS 等原生依赖。release 回归与最终程序共用优化依赖，ELF 检查拒绝动态加载器及共享库依赖。运行镜像为 gcr.io/distroless/static:nonroot，不复制任何 ICU 动态库。两架构均需执行实际容器冒烟，不能仅以交叉编译成功作为验收。

ICU4X 的 compact number 和 language display name 接口仍含不稳定 API，版本与数据通过 Cargo.lock 固定；升级时重跑 locale-format-vectors.json。该文件为公开格式化操作数，与站点挑战无关。与旧系统 ICU 对照的 109 组中，三组日语纽约时区名称采用 CLDR 48 的米国東部写法，其旧值和来源链接保留于 fixture；其余字段不放宽断言。
