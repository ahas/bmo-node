[English](README.md) | [한국어](README.ko.md) | [日本語](README.ja.md) | **简体中文**

# bmo-node

bmo 的节点服务:管理从这里可以到达的机器,以及关于这些机器的已知信息。

- **节点列表:** ssh 机器和各个 hub 的代理组成的一份列表,供其他所有单元读取;每个节点每分钟检查一次是否有响应
  (`docs/nodes.md`)。
- **硬件监控:** 通过每台机器各自的通路实时读取,每秒一次,并以分钟级和半小时级的行保留一个月(`docs/hardware.md`)。
- **本机:** 每分钟读取一次本机的状态,内存或磁盘压力持续存在时以事件通知(`docs/machine.md`)。是否可以启动某项任务,
  由调用方按自己的策略决定。
- **隧道入口:** `llm-bridge`,节点的隧道通过 HTTPS 接入这里(`docs/llm-bridge.md`)。
- **托管工具:** `bmo/node_exec`,为拥有该节点的账户下的调用方,或持有管理员授权的调用方,在 hub 节点上执行命令
  (`docs/tools.md`)。
- **运行(run):** 非会话单元以自身身份在 hub 节点上执行的命令;仅当节点自身的设置允许,且该节点属于它所代表的人时才会
  执行(`docs/api.md`,「Runs」)。

版本是整个套件的版本,在构建时从 git 标签读取(`docs/api.md`,「Versions」)。

## 目录结构

| 文件夹 | 内容 |
|---|---|
| `crates/` | Rust crate:客户端、harness 插件、策略文件、存储、节点检查、硬件监控、隧道入口和服务器二进制 |
| `deploy/` | 服务单元 `bmo-node.service` 和 `bmo-llm-bridge.service` |
| `docs/` | 权威文档,以及生成客户端所依据的契约 `openapi.json` |
| `packages/` | 由 `docs/openapi.json` 生成的 TypeScript 客户端 `bmo-node-client` |
| `tools/` | 托管工具 `node-exec`:清单、schema、手册及其可执行文件的 crate |
| `tests/` | 全部测试;覆盖整个仓库的检查位于顶层,探针的测试夹具位于 `tests/hardware/fixtures` |

## 运行

- `bmo-node --config <file> serve` 运行服务;配置文件的各项设置见 `docs/api.md`,「Configuration」。
- `bmo-node --config <file> node` 列出并修改节点;`bmo-node --config <file> hardware-setup` 在需要的节点上部署
  Windows 传感器库。
- `bmo-node mcp --url <url>` 是 harness 插件的 MCP 服务器,令牌取自 `BMO_NODE_TOKEN`;`crates/plugin/skills` 下的
  `bmo-node` 技能告诉 harness 何时以及如何使用它。
- `deploy/` 中的单元在 systemd 用户管理器下运行服务和入口。

## 边界

`docs/api.md` 是完整的契约:服务响应的路由、发布的事件、发布的客户端和 harness 插件,以及配置。`docs/openapi.json`
以 OpenAPI 3.1 描述同一契约,由代码生成;Rust 客户端 `bmo-node-client`(`crates/client`)和 TypeScript 客户端
(`packages/node-client`)都以它为基础。服务不直接指名任何其他单元:运行会向配置中指定的所有者查询入口询问节点归属
(`docs/nodes.md`,「A person's node」),托管工具则向 agency 询问节点和调用方各自属于谁(`docs/tools.md`)。
`docs/store.md` 说明它保存什么、保存在哪里。

`PERSONAL-DATA.md` 规定了任何文件都不得包含的个人数据:属于谁、哪些算在内、这类值应当放在哪里,以及保证这一点的检查。

## 构建与测试

- `cargo check --workspace --all-targets`
- `cargo test --workspace`,同时运行覆盖整个仓库的检查。
- testbed 的快速检查,其中包括 `personal-data` 检查:若受跟踪的文件中含有保留示例域名以外的邮件地址、个人主目录路径、
  电话号码、令牌或私有网络地址,该检查即失败(`PERSONAL-DATA.md`)。
- `cargo fmt --all --check` 和 `cargo clippy --workspace --all-targets -- -D warnings`
- 在 `packages/node-client` 中先 `bun install`,再 `bun run check`
