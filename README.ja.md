[English](README.md) | [한국어](README.ko.md) | **日本語** | [简体中文](README.zh-CN.md)

# bmo-node

bmo のノードサービスです。ここから到達できるマシンと、それらについて分かっていることを扱います。

- **ノード一覧:** ssh マシンとハブのエージェントをまとめた 1 つの一覧で、他のすべてのユニットがこれを読みます。各ノードが
  応答するかを 1 分ごとに確認します(`docs/nodes.md`)。
- **ハードウェアモニター:** すべてのマシンをそれぞれの経路で 1 秒に 1 回ライブで読み取り、1 分単位と 30 分単位の行として
  1 か月保持します(`docs/hardware.md`)。
- **マシン:** このマシンの状態を 1 分ごとに読み取り、メモリやディスクの逼迫が続くとイベントとして通知します
  (`docs/machine.md`)。起動を始めてよいかどうかは呼び出し側が自分で決めます。
- **トンネルの入口:** `llm-bridge`。ノードのトンネルが HTTPS でここに接続します(`docs/llm-bridge.md`)。
- **ホスト型ツール:** `bmo/node_exec`。ノードを所有するアカウントの呼び出し元、または管理者の許可を持つ呼び出し元のために、
  ハブノード上でコマンドを実行します(`docs/tools.md`)。
- **ラン(run):** セッションではないユニットが、自分自身としてハブノード上で実行するコマンドです。ノード自身の設定が許し、
  そのノードがユニットの代理する人のものである場合に実行されます(`docs/api.md`、「Runs」)。

バージョン 0.1.0-rev.2。

## 構成

| フォルダ | 内容 |
|---|---|
| `crates/` | Rust クレート: クライアント、ハーネスプラグイン、ポリシーファイル、ストア、ノードチェック、ハードウェアモニター、トンネルの入口、サーバーバイナリ |
| `deploy/` | サービスユニット `bmo-node.service` と `bmo-llm-bridge.service` |
| `docs/` | 正典ドキュメントと、クライアント生成の元になる契約 `openapi.json` |
| `packages/` | `docs/openapi.json` から生成した TypeScript クライアント `bmo-node-client` |
| `tools/` | ホスト型ツール `node-exec`: マニフェスト、スキーマ、マニュアル、実行ファイルのクレート |
| `tests/` | すべてのテスト。リポジトリ全体にわたるチェックは最上位に、プローブのフィクスチャは `tests/hardware/fixtures` にあります |

## 実行

- `bmo-node --config <file> serve` でサービスを起動します。設定ファイルの項目は `docs/api.md`、「Configuration」に
  あります。
- `bmo-node --config <file> node` はノードの一覧表示と変更を行います。`bmo-node --config <file> hardware-setup` は、
  必要なノードに Windows センサーライブラリを配置します。
- `bmo-node mcp --url <url>` はハーネスプラグインの MCP サーバーで、トークンは `BMO_NODE_TOKEN` から読みます。
  `crates/plugin/skills` にある `bmo-node` スキルが、いつどう使うかをハーネスに伝えます。
- `deploy/` のユニットは、サービスと入口を systemd のユーザーマネージャーの下で動かします。

## 境界

`docs/api.md` が契約のすべてです。サービスが応答するルート、発行するイベント、公開するクライアントとハーネスプラグイン、
そして設定を記しています。`docs/openapi.json` はコードから生成した OpenAPI 3.1 形式の契約で、Rust クライアント
`bmo-node-client`(`crates/client`)と TypeScript クライアント(`packages/node-client`)はこれに基づいています。
サービスは他のユニットの名前を持ちません。ランは設定で指定された所有者の参照先にノードの持ち主を問い合わせ
(`docs/nodes.md`、「A person's node」)、ホスト型ツールはノードと呼び出し元がそれぞれ誰のものかを agency に問い合わせます
(`docs/tools.md`)。何をどこに保持するかは `docs/store.md` にあります。

`PERSONAL-DATA.md` は、どのファイルにも含めてはならない個人データを定めています。誰のデータか、何が該当するか、
その値が代わりに置かれるべき場所、そしてそれを守るチェックを説明します。

## ビルドとテスト

- `cargo check --workspace --all-targets`
- `cargo test --workspace`。リポジトリ全体のチェックもあわせて実行されます。
- testbed のクイックチェック。そのうち `personal-data` チェックは、追跡対象のファイルに予約済みの例示ドメイン以外の
  メールアドレス、個人のホームパス、電話番号、トークン、プライベートネットワークのアドレスがあると失敗します
  (`PERSONAL-DATA.md`)。
- `cargo fmt --all --check` と `cargo clippy --workspace --all-targets -- -D warnings`
- `packages/node-client` で `bun install` の後に `bun run check`
