[English](README.md) | **한국어** | [日本語](README.ja.md) | [简体中文](README.zh-CN.md)

# bmo-node

bmo의 노드 서비스입니다. 여기서 닿을 수 있는 머신들과 그 머신들에 대해 알려진 정보를 다룹니다.

- **노드 목록:** ssh 머신과 허브의 에이전트를 담은 하나의 목록으로, 다른 모든 유닛이 이 목록을 읽습니다. 각 노드가
  응답하는지 1분마다 확인합니다(`docs/nodes.md`).
- **하드웨어 모니터:** 모든 머신을 각자의 경로로 1초에 한 번씩 실시간으로 읽고, 분 단위와 30분 단위 행으로 한 달간
  보관합니다(`docs/hardware.md`).
- **머신:** 이 머신의 상태를 1분마다 읽고, 메모리나 디스크 압박이 이어지면 이벤트로 알립니다(`docs/machine.md`).
  실행을 시작해도 되는지는 호출하는 쪽이 스스로 정합니다.
- **터널 출입구:** `llm-bridge`. 노드의 터널이 HTTPS로 여기에 접속합니다(`docs/llm-bridge.md`).
- **호스팅 도구:** `bmo/node_exec`. 노드를 소유한 계정의 호출자나 관리자 권한을 받은 호출자를 위해 허브 노드에서
  명령을 실행합니다(`docs/tools.md`).
- **런(run):** 세션이 아닌 유닛이 자기 자신으로서 허브 노드에서 실행하는 명령입니다. 노드 자체 설정이 허용하고, 그
  노드가 유닛이 대신하는 사람의 것일 때만 실행됩니다(`docs/api.md`, 「Runs」).

버전은 세트의 버전이며, 빌드할 때 git 태그에서 읽습니다(`docs/api.md`, 「Versions」).

## 구성

| 폴더 | 내용 |
|---|---|
| `crates/` | Rust 크레이트: 클라이언트, 하네스 플러그인, 정책 파일, 저장소, 노드 검사, 하드웨어 모니터, 터널 출입구, 서버 바이너리 |
| `deploy/` | 서비스 유닛 `bmo-node.service`와 `bmo-llm-bridge.service` |
| `docs/` | 정본 문서와, 클라이언트 생성의 기준이 되는 계약 `openapi.json` |
| `packages/` | `docs/openapi.json`에서 생성한 TypeScript 클라이언트 `bmo-node-client` |
| `tools/` | 호스팅 도구 `node-exec`: 매니페스트, 스키마, 매뉴얼, 실행 파일의 크레이트 |
| `tests/` | 모든 테스트. 저장소 전체에 걸친 검사는 최상위에, 프로브의 픽스처는 `tests/hardware/fixtures` 아래에 있습니다 |

## 실행

- `bmo-node --config <file> serve`로 서비스를 실행합니다. 설정 파일의 항목은 `docs/api.md`, 「Configuration」에
  있습니다.
- `bmo-node --config <file> node`는 노드를 나열하고 변경합니다. `bmo-node --config <file> hardware-setup`은 필요한
  노드에 Windows 센서 라이브러리를 설치합니다.
- `bmo-node mcp --url <url>`은 하네스 플러그인의 MCP 서버이며, 토큰은 `BMO_NODE_TOKEN`에서 읽습니다.
  `crates/plugin/skills` 아래의 `bmo-node` 스킬이 하네스에게 언제, 어떻게 쓰는지 알려 줍니다.
- `deploy/`의 유닛은 서비스와 출입구를 systemd 사용자 매니저 아래에서 실행합니다.

## 경계

`docs/api.md`가 계약 전체입니다. 서비스가 응답하는 라우트, 발행하는 이벤트, 배포하는 클라이언트와 하네스 플러그인,
그리고 설정을 담습니다. `docs/openapi.json`은 코드에서 생성한 OpenAPI 3.1 형식의 계약이며, Rust 클라이언트
`bmo-node-client`(`crates/client`)와 TypeScript 클라이언트(`packages/node-client`)가 이를 바탕으로 합니다. 서비스는 다른
유닛의 이름을 직접 알지 않습니다. 런은 설정에 지정된 소유자 조회 경로에 노드의 주인을 묻고(`docs/nodes.md`,
「A person's node」), 호스팅 도구는 노드와 호출자가 각각 누구의 것인지 agency에 묻습니다(`docs/tools.md`).
무엇을 어디에 보관하는지는 `docs/store.md`에 있습니다.

`PERSONAL-DATA.md`는 어떤 파일에도 담아서는 안 되는 개인정보를 규정합니다. 누구의 정보인지, 무엇이 해당하는지, 그런
값이 대신 있어야 할 곳, 그리고 이를 지키는 검사를 설명합니다.

## 빌드와 테스트

- `cargo check --workspace --all-targets`
- `cargo test --workspace`. 저장소 전체 검사도 함께 실행됩니다.
- testbed의 빠른 검사들. 그중 `personal-data` 검사는 추적되는 파일에 예약된 예시 도메인 밖의 메일 주소, 개인 홈 경로,
  전화번호, 토큰, 사설 네트워크 주소가 있으면 실패합니다(`PERSONAL-DATA.md`).
- `cargo fmt --all --check`와 `cargo clippy --workspace --all-targets -- -D warnings`
- `packages/node-client`에서 `bun install` 후 `bun run check`
