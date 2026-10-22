# 第三方来源

主项目采用 GPL-3.0-only，见 LICENSE。第三方组件保留各自许可，汇总文本位于 LICENSES。

- HTTP/TLS：wreq、wreq-util 及其原生 TLS/压缩依赖；使用库提供的传输实现，不包含浏览器进程。
- 静态语法：tree-sitter 与 tree-sitter-javascript，MIT。只读取语法和有限表达式，不执行站点 JavaScript。
- 图形、文字宽度、变换与部分接口声明的有限软件模型，按 MIT 许可改编自 nokk 的公开实现与声明资料；许可见 LICENSES/nokk.txt。不导入 nokk，也不包含 JS/DOM 模拟执行环境。
- profiles/webidl-surface-v1.json 的名称、继承及成员声明来自 W3C Webref 的 @webref/idl 3.85.2，MIT，见 LICENSES/webref.txt。运行时不含 IDL 引擎。
- profiles/browser-api-declarations-v1.json 参考 nokk 的公开接口声明与属性顺序，是显式模型输入，不是录制的挑战答案。
- 实验性 JSD 字段分组参考 xKiian/cloudflare-jsd 的 jsd/fp.go，MIT，见 LICENSES/cloudflare-jsd.txt。UA、语言、网址等由当轮输入填入；此路径不代表严格原生验收。
- ICU4X 用于月份/时区名称、语言名称、列表及紧凑数字格式。采用 Rust 组件与内嵌 CLDR 数据，遵循 Unicode-3.0 许可，见依赖许可汇总；不包含 ICU4C 动态库。sys-locale 只负责读取系统语言。
- Linux 全静态构建包含 musl 及工具链运行库。musl 许可见 LICENSES/musl.txt；GCC 运行库采用 GPLv3 与 Runtime Library Exception 3.1，例外全文见 LICENSES/gcc-runtime-exception.txt，GPLv3 全文见 LICENSE。
- chrono-tz 依赖的时区数据许可见 LICENSES/tzdata.txt。
- Wasmtime 仅运行匹配已审核摘要的有限算术模块，禁用外部导入并限制内存、指令与调用次数，相关许可见依赖汇总。

tests/fixtures 中的编解码和常量向量由开发时的隔离参考运算生成，使用公开数据和自定义测试密钥，不包含 Cookie 或账号会话。python-contracts.json.gz 为旧实现纯数据契约，Rust 测试直接读取，不运行 Python。挑战初始化数据仅用于离线静态解析回归；运行服务从当前响应恢复参数，不读取旧任务结果。

本分支不交付旧 Python 解释器、Python ABI、curl_cffi、NumPy、nokk 或开发抓包。Rust 完整依赖许可汇总见 LICENSES/rust-service-dependencies.txt。
