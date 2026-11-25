# moldette

[English](README.md) | 한국어

이 문서는 `README.md`의 번역이다. 내용이 다르면 영어판을 따른다.

moldette는 원본 레코드를 사용자가 선언한 스키마의 레코드로 변환한다. YAML
또는 JSON 파일 하나에 레코드를 읽는 방법(JSON, CSV를 비롯한 구분자 텍스트,
정규식으로 읽는 로그 줄, `key=value` 쌍), 출력 필드마다 값을 채우는 방법,
필수 필드를 기술한다. moldette는 이 파일을 한 번 검사한 뒤 레코드를 하나씩
처리하고, 레코드마다 스키마에 맞는지와 맞지 않는 이유를 알려 준다.

한 번에 레코드 하나만 처리한다. 데이터 수집, 저장, 중복 제거, 외부 데이터
조회, 여러 레코드에 걸친 처리(집계, 조인)는 호출하는 쪽에서 한다.

## 빠른 시작

`moldette` 명령은 [GitHub Release](https://github.com/CharmingGroot/moldette/releases)에
첨부된 바이너리로 설치하거나 Cargo로 설치한다.

```bash
cargo install moldette-cli
```

설정 파일 `order.yaml`을 작성한다. 출력 스키마를 선언하고, JSON 레코드에서
필드 네 개를 읽어 이름이 같은 스키마 필드를 채운다.

```yaml
schemas:
  - name: order
    version: 1
    fields:
      - {name: id, type: string, required: true}
      - {name: status, type: string}
      - {name: total, type: float, required: true}
      - {name: created_at, type: timestamp, required: true}

parser:
  format: json
  fields:
    id: /id
    status: /status
    total: /total
    created_at: /created_at

mapper:
  schema: {name: order, version: 1}

resp:
  schema: {name: order, version: 1}
```

설정을 검사한 뒤, 한 줄에 하나씩 있는 레코드에 실행한다.

```bash
moldette check order.yaml
moldette run order.yaml orders.ndjson
```

```text input
{"id":"A-1","status":"paid","total":25.5,"created_at":"2026-11-18T09:30:00Z"}
{"id":"A-2","total":"n/a","created_at":"2026-11-18T10:00:00+09:00"}
not json
```

```json output
{"status":"ok","fields":{"id":"A-1","status":"paid","total":25.5,"created_at":"2026-11-18T09:30:00Z"},"emptied":[]}
{"status":"missing_required","fields":["total"]}
{"status":"malformed","reason":"expected ident at line 1 column 2"}
```

두 번째 레코드의 `total`은 숫자가 아니어서 필수 필드에 값이 없다. 세 번째는
JSON이 아니다. `moldette explain order.yaml orders.ndjson`은 필드마다 무엇을
읽었고 어떻게 변환했는지 보여 준다.

## 동작 방식

레코드는 세 레이어를 거친다. 세 레이어 모두 설정 파일로 기술한다.

- **parser**는 원본 레코드를 이름 붙은 텍스트 필드로 읽는다. 다른 형식의
  텍스트를 담은 필드, 예를 들어 구분자 헤더 뒤의 JSON 본문은 한 번 더 파싱할
  수 있다.
- **mapper**는 그 필드들로 출력 스키마의 필드를 채운다. fallback, 기본값,
  상수, 작은 op 집합(trim, split, extract, lookup, timestamp 파싱)을 쓸 수
  있고, 텍스트를 string, int, float, bool, timestamp, IP 주소로 변환한다.
- **resp**는 결과를 스키마와 대조해 레코드에 상태 하나를 붙인다. `ok`,
  `malformed`, `missing_required`, strict 모드에서는 `emptied`다.

설정은 읽을 때 검사한다. 알 수 없는 키, 레이어 사이의 참조, 규칙 오류는 레코드를
처리하기 전에 위치와 함께 보고한다. 표현식 언어는 없고, 사용자 정의 parser와
op는 Rust 타입으로 등록한다. 설정에서 출력 필드마다 어느 입력 필드에서 왔는지,
즉 리니지(lineage)도 얻을 수 있다.

## 사용하는 세 가지 방법

**`moldette` 명령**은 설정을 검사하고, 파일이나 표준 입력에 실행한다. 입력
순서를 유지하면서 모든 코어를 사용한다. `--records json`은 JSON 값 또는
최상위 배열의 원소를 레코드로 읽고, `--records multiline`은 여러 줄을 레코드
하나로 합친다. `.gz` 파일은 압축을 풀어 읽는다.

**`moldette-sidecar`**는 애플리케이션과 같은 Pod나 호스트에서 설정을 실행한다.
애플리케이션은 레코드를 HTTP로 보내고, 응답이 만들어지는 대로 읽는다. 사이드카
하나로 이름 붙은 설정 여러 개를 실행할 수 있고, 설정이 바뀌면 HTTP 주소에서
다시 읽는다. 상태 확인(probe)과 Prometheus 메트릭을 제공하며, 컨테이너 이미지와
Kubernetes 예시가 있다. `docs/SIDECAR.md`를 참고한다.

**`moldette` crate**는 같은 파이프라인을 Rust 프로그램에 제공한다. `yaml`에
설정이, `raw`에 레코드 하나가 있을 때 다음과 같이 쓴다.

```rust
use moldette::Pipeline;
use moldette::config::{Config, Registry};

let specs = Config::from_yaml(yaml)?.build(&Registry::new())?;
let pipeline = Pipeline::from_specs(&specs)?;
let response = pipeline.process(raw);
```

빌드한 파이프라인은 변경되지 않으므로 여러 스레드가 공유할 수 있다. 레이어는
각각 따로 쓸 수도 있다. 예를 들어 레코드를 매핑하지 않고 파싱만 하거나, 다른
곳에서 만든 레코드의 필수 필드만 검사할 수 있다.

## 설치

| 대상 | 방법 |
|---|---|
| 라이브러리 | `cargo add moldette` |
| 명령 | `cargo install moldette-cli`, 또는 GitHub Release의 바이너리(Linux x86_64·aarch64, macOS x86_64·aarch64, Windows x86_64) |
| 사이드카 | `cargo install moldette-sidecar`, 또는 이미지 `ghcr.io/charminggroot/moldette-sidecar`(`linux/amd64`, `linux/arm64`) |
| Claude Code skill | `/plugin marketplace add CharmingGroot/moldette` 후 `/plugin install moldette@moldette` |

릴리스 바이너리에는 `SHA256SUMS` 파일이 함께 있고, 바이너리와 이미지에는 빌드
출처 증명(build provenance attestation)이 있다.

    gh attestation verify moldette-cli-0.8.0-x86_64-unknown-linux-musl.tar.gz -R CharmingGroot/moldette

`moldette` crate의 기본 feature인 `config`는 모든 레이어를 켠다. 설정 파일 없이
레이어 하나만 쓰려면 `moldette-parser`처럼 그 레이어의 crate에 의존하거나, 기본
feature를 끄고 레이어 feature를 고른다.

## 문서

문서는 영어로 제공한다.

- `docs/CONFIGURATION.md`: 모든 설정 키, 형식, op. 예제는 테스트로 실행한다.
- `docs/TROUBLESHOOTING.md`: 응답 상태, 필드에 값이 없는 이유를 찾는 방법,
  `moldette check` 메시지.
- `docs/SIDECAR.md`: 사이드카 실행, 여러 설정, URL에서 읽는 설정, 상한, 메트릭,
  Kubernetes.
- `examples/configs`: nginx access log, syslog, JSON API 응답, CSV 내보내기의
  설정과 입력, 출력.
- `examples/otlp`: 파이프라인의 span을 OpenTelemetry collector로 보내는 예시.
- `plugins/moldette`: 설정 작성, 검사, 배포를 돕는 Claude Code skill. 위 문서를
  바탕으로 한다.
- `CHANGELOG.md`: 릴리스별 변경과 호환성 규칙.

## Crates

| Crate | 내용 |
|---|---|
| `moldette` | 파이프라인. 레이어마다 같은 이름의 feature로 고른다. 레코드 구분(framing) |
| `moldette-config` | 설정 파일, 섹션 사이의 검사, 리니지 |
| `moldette-parser`, `moldette-mapper`, `moldette-resp` | 레이어 하나씩 |
| `moldette-core` | 레이어가 주고받는 타입 |
| `moldette-cli` | `moldette` 명령 |
| `moldette-sidecar` | HTTP 사이드카 |
| `moldette-spec` | `serde`를 이용한 응답 출력 형식 |

레이어 crate는 `moldette-core`에만 의존한다. 라이브러리 crate는 I/O를 하지
않고 전역 상태를 두지 않으며, 모두 `wasm32-unknown-unknown`으로 빌드된다.
`tracing` feature를 켜면 레이어가 `tracing` span과 event를 내보내고, 내보낸
신호의 수집은 호출하는 쪽에서 한다.

## 상태

moldette는 0.x 버전이고 Rust 1.88 이상이 필요하다. minor 릴리스는 공개 API,
설정 형식, 명령과 사이드카의 출력을 바꿀 수 있고, patch 릴리스는 바꾸지 않는다.
변경은 `CHANGELOG.md`에, 범위와 레이어, 마일스톤은 `docs/PLAN.md`에 있다.

## 기여

이슈는 환영한다. pull request는 유지자가 할당한 이슈에 대해서만 받는다.
`CONTRIBUTING.md`를 참고한다. 취약점은 `SECURITY.md`에 따라 신고한다. 참여는
`CODE_OF_CONDUCT.md`를 따른다.

## 라이선스

Apache-2.0. `LICENSE`를 참고한다.
