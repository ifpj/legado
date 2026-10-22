# 开发与维护

本分支为独立 Rust 工程，根目录就是 Cargo 工程。原 Python 参考实现、调试抓包、真机脚本、构建目录和个人运行配置不进入本分支。

## 模块

| 模块 | 职责 |
| --- | --- |
| web / assets | Axum 路由、静态 Web、访问令牌与请求历史界面 |
| service | 会话隔离、32 个会话上限、四路读取、独占验证、超时和取消统计 |
| network | 原生 HTTP/TLS、重定向、Cookie、DoH 元数据、CDN 地址映射与有界回退 |
| engine | 从初始请求到独立复读目标页面的完整协议流程 |
| constants / compiler / compiler_profiles / data | 静态语法、常量恢复、有限模型和数据校验 |
| catalog / models / model_geometry / surface / locale / semantics / declared | 按当轮输入计算已识别任务 |
| wire / completion / http_tasks | 编解码、后续阶段判定、受约束的 HTTP 子任务 |
| sources / assets/*-parser.js | 动态书源导出及各站点 HTML 解析 |

profiles 是显式声明、公式和不可变协议布局，不是已计算的浏览器指纹或可复用验证凭证。当前软件模型不声称测量了真实浏览器或发生了人工交互。

Wasmtime 只用于匹配已审核摘要、无外部导入且带内存/指令/调用限制的有限算术模块，不是任意网站程序运行入口。Tree-sitter 用于读取语法，不执行 JavaScript。

## 修改约束

1. 先定位失败阶段。HTTP 200、Cookie 颁发、工作量计算完成都不等于取得目标网页。
2. 不能使用旧 Cookie、录制答案、固定任务结果来隐藏协议失败。未知格式应保留明确错误。
3. 不添加网页响应缓存。DNS 元数据和精确程序布局可复用，但不得携带旧密钥、任务输入或答案。
4. 同一总时限覆盖排队、解析、验证、重定向与有限回退，不能用无界重试伪造稳定性。
5. 新的辅助函数必须进入实际运行路径后再用于对照，避免只在测试中“实现”兼容。
6. 不改阅读应用来掩盖示例解析问题。示例站点应在指定的真实阅读环境中验收。
7. 抓包默认关闭，提交前检查 fixture 和文档，不提交个人令牌、代理密码、账号会话、响应抓包和绝对开发路径。

## 检查

```sh
cargo fmt --check
cargo test --locked
for file in assets/*.js tests/*.cjs; do node --check "$file"; done
docker build -t cf-gateway:test .
```

Rust 测试不访问真实网站。tests/smoke-container.cjs 需要先启动容器，CI 使用宿主网络让容器访问临时本地上游；脚本不依赖外网和账号。CI 分别在原生 amd64、arm64 runner 构建、运行测试、验证实际容器，然后合并镜像清单。

本机手动测耗时应采用 release 构建，分别记录冷进程/新会话/复用会话。保留全部失败与慢样本；记录墙钟总耗时和服务内部耗时，不能将本地计算时间冒充端到端时间。真实网站验收必须检查目标页面的业务标记；例如 Linux.do 测试地址的真实网站页面可能是 404，不能只以 HTTP 200 判断。

## 错误复现

Web“请求历史”可筛选处理失败，查看 code、error、details、排队和总耗时。点击“填入工作台”后手动执行；原会话可能已经变化，可以同时比较原会话和新会话。历史只保留 200 条和复现所需参数，不保存页面正文、令牌、Cookie 或代理账号密码。含账号代理需要重新输入。

--capture-dir 仅用于显式本地诊断；输出可能包含短期挑战状态，不能直接上传。用于报告的 URL 应移除临时验证参数。不要为了调试绕过 TLS 证书校验或修改系统代理配置。

## 平台

Windows 使用系统 ICU；Linux 动态查找 libicui18n.so 及版本化符号。Docker 在 Debian 12 构建，并将 ICU 72 运行库复制到 distroless/cc-debian12。不能直接换成 distroless/static 并删掉 ICU：日期、数值与排序模型会缺失依赖。升级基础发行版时同时检查 ICU 路径和两种架构的容器测试。
