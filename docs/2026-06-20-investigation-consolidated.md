# AI Usage Tracker — 통합 조사 리포트 & 실행 로드맵

- **작성일**: 2026-06-20
- **범위**: 조사 + 통합 계획만 (이 라운드에서 코드 변경 없음). 구현은 별도 라운드.
- **방법**: 8개 영역을 각각 별도 에이전트로 병렬 조사 (보안 / 크로스-OS / 리팩토링 / UI·UX / 테스트·CI / 패키징·서명 / 성능·번들 / 접근성·i18n). 각 에이전트는 read-only.
- **검증 한계**: macOS만 실검증 가능. Windows/Linux 관련 주장은 모두 `[검증필요]` — context7 Tauri 2 문서 + 정적분석 기반.

---

## 1. 종합 요약

코드베이스는 **백엔드 단위 테스트·격리 설계·보안 핵심(토큰 비유출)** 측면에서 기반이 탄탄하다. 진짜 격차는 네 곳에 집중된다:

1. **"실데이터만" 원칙을 UI가 일부 위반** — 가짜 타임스탬프(`recentActivity()`)·가짜 SaaS 내비("Control Plane / Workspace / Members / Billing")·하드코딩된 "Updated just now". 리디자인의 핵심 명분.
2. **크로스-OS가 5/6 provider는 동작하지만 Copilot 자격증명 + 트레이 UX가 Windows/Linux에서 깨짐**. CI가 전혀 없어(`.github/` 부재) Win/Linux는 빌드조차 검증된 적 없음.
3. **항상 켜져 있는 메뉴바 앱인데 1초마다 전체 트리 리렌더**(메모이제이션 0) — 영구적 idle CPU/배터리 낭비.
4. **배포 준비 0** — 서명·공증·업데이터·릴리즈 파이프라인 전무. 현재 산출물은 전부 미서명(Gatekeeper/SmartScreen 경고).

이 라운드의 권고: **UI 리디자인은 "Cockpit"(잔여 헤드룸 게이지) 방향을 추천**, 구현 착수 전 §5의 결정사항부터 확정.

---

## 2. 교차 발견 (복수 에이전트 공통 지적 — 최우선 신뢰)

| 발견 | 지적한 에이전트 | 핵심 | 위치 |
|---|---|---|---|
| **회전 토큰 write 실패 무시 → 계정 락아웃** | 보안 + 리팩토링 | `let _ = write_back(...)` ×4. Codex는 single-use 회전이라 write 실패 시 확정 락아웃 | `providers/mod.rs:77`, `claude.rs:499,530`, `codex.rs:279`, `gemini.rs:195` |
| **가짜 데이터 — "실데이터만" 위반** | UI + 리팩토링 | `recentActivity()`가 타임스탬프 날조; "Updated just now" 하드코딩; 가짜 워크스페이스 내비; `raw_response`는 IPC까지 가지만 렌더링 안 됨 | `Dashboard.tsx:1461,294,379` |
| **죽은 `sort_index`/reorder/custom-sort** | 리팩토링 + 접근성 | 드래그 재정렬 UI가 아예 없음. `sort_index`는 항상 canonical 인덱스만. "Custom" 정렬은 항상 canonical 결과 | `providers.ts:49`, `types.ts:58` |
| **CI 전무** | 테스트 + 패키징 | `.github/` 디렉터리 자체가 없음. 66 Rust + 9 TS 테스트가 Win/Linux에서 한 번도 안 돌아감 | (저장소 전체) |
| **평문 토큰 at-rest** | 보안 + 크로스-OS | `accounts.json`에 Gemini refresh+id_token, Copilot `gho_`, z.ai 키가 평문. 백업/클라우드싱크 유출면 | `store.rs:49-57` |

---

## 3. 영역별 핵심

### 3.1 보안 (Severity: 전반 양호, P0 1건)
- **P0 불변식 2개 모두 성립** ✅ — 토큰은 IPC를 안 넘김(`AccountInfo`는 `{id,provider,label}`만), 공개 client_id만 사용. 단 `raw_response`는 토큰 없는 PII(이메일/플랜)를 webview heap까지 전달(렌더링은 안 함).
- **최대 리스크**: 평문 토큰 at-rest(Med). macOS는 부모 디렉터리 `0700`이라 타 유저 차단되지만 백업/싱크/동일유저 악성코드에 노출. `accounts.json`은 `0644`로 기록(Linux에서 other-read 노출 가능).
- **하드코딩 Google client_secret** `GOCSPX-...`(`oauth_login.rs:11`)는 Gemini CLI의 **공개 installed-app secret**(비기밀, env override 가능) → 실위험 Low. 코드 주석만 권고.
- TLS 견고(rustls, `danger_accept_invalid_certs` 없음). 토큰 로깅 없음 ✅.
- 기타: webview **CSP 없음**(`csp: null`); 세션키 입력 필드 미마스킹(`type="password"` 아님); `login.rs`/`oauth_login.rs` 에러 경로는 `http.rs`의 HTML 리댁션 미적용.
- **권고**: P0 write-실패 처리 / P1 OS 키체인 이전 + 임시 chmod 0600 + CSP / P2 입력 마스킹·에러 리댁션 일관화·`raw_response` 축소.

### 3.2 크로스-OS 지원 (5/6 provider 이식 가능, 2개 진짜 격차)
- **파일 기반(Codex/Gemini/z.ai)·Cursor SQLite는 이식 OK** — `dirs` 크레이트가 Win `%APPDATA%`/Linux XDG로 정확히 매핑. 경로 분리자·home 해석 문제 없음.
- **Claude는 Win/Linux에서도 정상** — mac만 키체인, Win/Linux는 평문 `~/.claude/.credentials.json`. 현재 파일 폴백이 **올바름** (기존 "키체인 read가 mac 전용이라 깨짐" 가설은 오답).
- **[P0] Copilot 자동감지가 Win/Linux 키스토어에서 실패** — Copilot CLI는 3-OS 모두 OS 네이티브 시크릿 저장소(Win Credential Manager / mac Keychain / Linux libsecret) 사용, 파일은 폴백일 뿐. 현재 코드는 **mac에서만** 키스토어를 읽음 → 정상 Win/Linux 박스에서 `NotLoggedIn`. `COPILOT_HOME`도 미지원.
- **[P1] 트레이/팝오버가 macOS 메뉴바 전제** — `lib.rs:98-106` 좌표 수학이 화면 상단 가정. Windows는 작업표시줄(우하단), Linux는 좌클릭-팝오버 토글이 StatusNotifier에서 비신뢰. → `tauri-plugin-positioner` + Linux는 컨텍스트 메뉴 1차 경로.
- **[P2] 빌드 검증 없음** — `targets: "all"`이지만 host OS만 번들. `#[cfg]` 분기·`rusqlite bundled`가 Win/Linux로 컴파일된 적 없음.
- **권고**: Copilot 키스토어 읽기는 **"완전 자동감지 패리티"가 목표면 P0, 수동 PAT 붙여넣기 허용이면 P2**(결정 필요). `keyring` 크레이트 우선 시도 → libsecret/Credential Manager 스키마 불일치 시 `secret-tool`/FFI 폴백.

### 3.3 전체 리팩토링 (행동 보존 전제)
- **"provider 추가 = union 한 줄"이 프론트는 지켜지나 Rust 백엔드는 위반** — `build_providers`(6 if), `fetch_credential`(6-arm), `refresh_stored`(6-arm), 그리고 서로 호환 안 되는 `fetch_with` 시그니처 4종. **해법의 선례가 이미 저장소에 있음**: `oauth_login.rs::spec_for(Provider) -> OAuthSpec` 테이블 패턴을 fetch/refresh 계층으로 확장.
- **Dashboard.tsx(1520줄)는 모놀리식 렌더가 아니라 25개 컴포넌트 + 4 훅이 한 파일에 동거** — 대부분 기계적 파일 분리 + `useThresholdToasts` 추출이면 오케스트레이터가 ~150~200줄로 축소.
- **OAuth refresh 삼종세트 복붙**(struct `Refreshed` ×3, `build_refreshed_cred`/`apply_refresh` ×3, `let _ = write_back` ×4, `capitalize` ×3, JWT claim 추출 ×2) → 공유 모듈로.
- **`LimitWindow` 손수 17회 / `ServiceUsage` 13회 생성**(항상 동일한 5~6 필드) → 생성자 + `#[derive(Default)]`.
- **severity 밴드가 Dashboard 인라인 5회 재구현**되는데 정작 `percentSeverity`/`severityToStatus`는 미사용 → 단일 출처 버그 위험.
- **행동 변경 항목(별도 결정 필요, 리팩토링 아님)**: gemini `write_back_creds`가 항상 `Ok`; zai `Status{status:200}`; SettingsDialog 6개 무동작 토글; `raw_response`/`sort_index`/reorder 미연결; placeholder `onClick`. → "정리"하면 관측 가능 행동이 바뀌므로 의도적 결정으로 분리.
- **IPC 계약 수동 동기화(`model.rs` ↔ `types.ts`)** — enum 재정렬 시 잘못된 provider config를 읽어도 타입 에러 없음 → 경계 런타임 assertion 권고.

### 3.4 UI/UX 리디자인 (3개 방향, "Cockpit" 추천)
- **현 UI 비평**: 숫자는 정직(tnum/`.num` 좋음), a11y 기반(focus-visible, reduced-motion, 트레이 progressbar) 양호. 하지만 "AI-default 다크 대시보드"로 익명적; 가짜 SaaS 내비("Control Plane v1.2.0")·날조된 Recent Activity·장식용 macOS 신호등이 "templated" 느낌의 원인.
- **방향 A — Cockpit (추천)**: 잔여 **헤드룸 게이지**(소진형 라디얼). "8% 남음, 0:42 후 리셋"이라는 *실제 의사결정*으로 프레임 전환. 다크 계기판. severity = 글리프(○◐●) + 호 두께 + 모션 3중. 하나의 메타포가 트레이→카드→모달 전 표면에 스케일.
- **방향 B — Ledger**: 페이퍼-화이트 모노스페이스 터미널, `[████░░──] 92% CRIT` 브래킷 미터. 자기설명적(grayscale에서도 읽힘) → **크로스-OS 최안전·최저비용**, 트레이 밀도 최고.
- **방향 C — Almanac**: 색온도+텍스처로 "날씨"처럼 표현(◍ STORM/◔ HAZY/○ CLEAR). 가장 독창적이나 **a11y 비용 최대**(색 의존)·serif 폰트 의존 → 위험.
- **모든 방향 공통 권고(어느 걸 선택하든)**: `recentActivity()` 제거(날조), "Updated just now"를 실 `fetchedAt`에 바인딩, 가짜 워크스페이스 내비·신호등 제거 → "templated" 인상의 최대 원인 제거. `.num`/focus-ring/progressbar/reduced-motion 유지.
- **추천 + 위험**: A 채택. 위험 = 32px 트레이 게이지 가독성(텍스트 폴백 병행), FLIP 전환 jank(reduced-motion 폴백), 시안 액센트가 severity 색에 침범 금지.

### 3.5 테스트 & CI/E2E (단위층 양호, 그 외 0)
- 현황: **Rust 66 통과 + TS 9 통과**, 전부 순수함수·node 환경. DOM/렌더/HTTP/IPC 테스트 0. **CI 0**.
- **최대 격차 = CI 부재**. 초록 스위트를 Win/Linux에서 돌리는 워크플로 구축이 신규 테스트 작성보다 우선(P0).
- HTTP 경로(fetch/refresh/error) 구조적으로 미테스트 — base URL이 `const`라 주입 불가. `http::decode_json`(Cloudflare-HTML 새니타이저)이 미테스트 분기 최다.
- 계약 드리프트 가드 없음 → `ts-rs`(codegen) 또는 golden-JSON shape 테스트.
- **E2E 2계층 권고**: (A) `tauri-driver`+WebdriverIO = 실 IPC 스모크(**Linux+Windows만**, macOS는 WKWebView WebDriver 미지원), (B) Playwright vs `vite preview` = 브라우저-런타임 폴백(`hasTauriRuntime()`) 활용해 **3-OS(macOS 포함) UI E2E**. macOS E2E 답이 이미 코드에 내장됨.
- 제안 워크플로 3종(ci / build-smoke / e2e) — 상세 YAML은 테스트 에이전트 리포트 참조. **빌드는 `--debug --no-bundle` 스모크만**(서명·번들은 패키징 영역).

### 3.6 패키징·배포·코드서명 (배포 준비 0)
- 현황: 로컬에서 미서명 `.app`+`.dmg`(aarch64)만. 서명·공증·업데이터·`createUpdaterArtifacts`·per-OS 번들 블록·릴리즈 워크플로 전부 부재.
- **사용자가 직접 확보해야 할 것(비용·리드타임)**:
  - macOS: Apple Developer ~$99/년 + Developer ID Application 인증서 + 앱암호(공증). (P0 차단)
  - Windows: **Azure Trusted Signing ~$10/월 권장**(CI 친화) 또는 OV/EV 인증서 ~$200~600/년(EV는 HW 토큰으로 CI 난해). 조직 신원검증은 수일~수주 소요.
  - Linux: 유료 신원 불필요.
  - 업데이터: minisign 키쌍(무료, **개인키 분실 시 설치본 업데이트 영구 불가**).
- **기능 격차**: 메뉴바 앱이라면서 `set_activation_policy(Accessory)` 미호출 → mac에서 Dock 아이콘 노출; single-instance·autostart 없음.
- **비이슈**: `omp-session-*.html`는 이미 gitignore(미추적), 아이콘 전 타겟 완비, `Cargo.lock` 커밋됨.
- 권고: P0 macOS 서명·공증 우선 / P1 Win·Linux CI 빌드 / P2 자동업데이트·autostart.

### 3.7 성능 & 번들
- **[P0] 1초마다 전체 트리 리렌더** — `useNow(1000)`를 루트에서 호출 + `nepMs`를 ~20 자식에 전달 + `React.memo` 전무. 항상 켜진 메뉴바 앱에 영구 1Hz 재조정. → 메모이제이션 / 타이머를 리프로 격리 / 15~30s로 완화.
- **[P0] 팝오버 webview가 풀 번들 로드** — 숨은 팝오버가 351KB JS를 파싱·보유하며 150줄 컴포넌트만 렌더. → `App.tsx`에서 `React.lazy()` 분리.
- **[P0] `[profile.release]` 부재** — `lto`/`opt-level="z"`/`strip`/`panic="abort"` 표준 최적화 누락(바이너리 크기). 런타임 위험 0.
- **[P1] reqwest 클라이언트 매 fetch 생성**(6+N개/사이클 폐기) — 300s 주기라 절대 비용은 작음. 정합성/베스트프랙티스 개선(S).
- **[P1] 폰트 5개 전 subset 번들**(~172KB) → `latin.css`로 교체(바이너리 크기; 런타임은 `unicode-range`로 이미 미로드).
- **[P2] `raw_response`를 팝오버에 전 broadcast**(읽지도 않음, ~6KB/사이클) → compact + inspector open 시 fetch.
- 번들: `index.js` 351KB raw / **108KB gzip**(react-dom 대부분). lucide 트리셰이킹 정상, 독립 폴링 아님(스케줄러 1개·양 창에 broadcast), 첫 fetch는 spawn이라 창 표시 비차단.

### 3.8 접근성 & 국제화
- **a11y 강점**: 카드가 실제 `<button>`, Radix Dialog 포커스 트랩, 전역 `:focus-visible`, 완전한 reduced-motion, 트레이 progressbar, severity가 색 단독 아님(퍼센트 텍스트 동반).
- **약점**: Dashboard 막대가 `role="progressbar"` 없음(시각 전용); **live region 없음**(스냅샷 갱신·에러가 SR에 무음); 커스텀 kebab 메뉴(`Dashboard.tsx:892`)가 menu role/화살표/Escape/외부클릭 전무 → **Radix DropdownMenu로 교체**; 아이콘 전용 버튼 다수 미라벨.
- **대비 실패(체계적)**: `--text-faint` #737780 = 3.04~3.92:1, 11~12px 본문에 광범위 사용 → 4.5:1 미달. `--text-dim`은 통과. → `--text-faint` 상향.
- **i18n 준비 ≈ 0**: ~100 하드코딩 영문 + Rust 5개 영문 에러. **가장 어려운 부분 = 백엔드 에러 문자열**(`ProviderError` Display가 IPC로 free text, 에러 코드 없음). → Rust에 안정적 `code` 추가 후 `{code, detail}` 전달, 프론트가 `code→t()` 매핑. **react-i18next + en/ko + Tauri `plugin-os` 로케일 시드** 권고. `format.ts`의 `.replace("Updated ","")`는 비영어에서 깨짐 → `Intl.*`로 교체.

---

## 4. 통합 우선순위 로드맵

> 효과/안전/의존을 고려해 영역을 가로질러 재배열. 효과 표기: S/M/L.

### P0 — 기반·안전·최고 ROI (먼저)
| # | 항목 | 영역 | 효과 | 비고 |
|---|---|---|---|---|
| 1 | 회전 토큰 write 실패 처리(무시 제거, 실패 시 옛 토큰 유지) | 보안 | S | 락아웃 방지. Codex 최우선 |
| 2 | CI 구축(`ci.yml`+`build-smoke.yml`: 3-OS cargo test/clippy/fmt, vitest, tsc, `tauri build --debug --no-bundle`) | 테스트 | M | Win/Linux 검증 유일 수단 |
| 3 | 1Hz 리렌더 해소(memo/타이머 격리 또는 15~30s) + 팝오버 `React.lazy` + `[profile.release]` | 성능 | S~M | 항상 켜진 앱의 최대 런타임 낭비 |
| 4 | UI 정직성 수정(가짜 activity 제거, `fetchedAt` 바인딩, 가짜 내비·신호등 제거) | UI/리팩토링 | S | "실데이터만" 위반 제거 = 리디자인 선결 |
| 5 | webview CSP 추가(`default-src 'self'`) | 보안 | S | 방어심층 |
| 6 | Copilot Win/Linux 키스토어 읽기 **(자동감지 패리티가 목표일 때)** | 크로스-OS | M | 결정-게이트(§5.2) |

### P1 — 크로스-OS 커버리지·핵심 개선
| # | 항목 | 영역 | 효과 |
|---|---|---|---|
| 7 | OS 키체인으로 토큰 이전(+임시 chmod 0600) | 보안/크로스-OS | M~L |
| 8 | 트레이 위치 `tauri-plugin-positioner` + Linux 컨텍스트메뉴 1차 경로 | 크로스-OS | S~M |
| 9 | macOS 서명·공증 + `bundle.macOS hardenedRuntime` + Accessory 정책 + single-instance | 패키징 | M (인증서 필요) |
| 10 | 리팩토링 1차(죽은코드 제거, `LimitWindow`/`ServiceUsage` 생성자, refresh 삼종세트 통합, Dashboard 파일분리, severity 단일출처) | 리팩토링 | M |
| 11 | 계약 드리프트 가드 + 프론트 순수로직 테스트 + wiremock HTTP 커버리지(URL 주입) | 테스트 | M~L |
| 12 | a11y(progressbar role, Radix DropdownMenu, live region, `--text-faint` 대비) | 접근성 | M |
| 13 | i18n(react-i18next + en/ko + Tauri 로케일 + `format.ts` Intl 화 + 백엔드 에러 코드) | i18n | M~L |

### P2 — 배포 완성·아키텍처 페이오프·폴리시
| # | 항목 | 영역 | 효과 |
|---|---|---|---|
| 14 | Windows 서명(Azure Trusted Signing) + Linux 번들 + 자동업데이트 + autostart | 패키징 | M~L (비용/결정 게이트) |
| 15 | provider 레지스트리/`FetchSpec` + `fetch_stored` 통일 (escape hatch 필수) | 리팩토링 | L (마지막에, 테스트 통과 게이트) |
| 16 | 폰트 latin subset + `raw_response` compact·broadcast 제외 + vendor 청크 | 성능 | S |
| 17 | E2E(Playwright UI + tauri-driver 스모크), `<main>`/`<h1>`, 제거확인 다이얼로그 등 | 테스트/접근성 | M~L |

---

## 5. 사용자 결정 필요 사항 (구현 착수 전 확정)

1. **토큰 저장**: OS 키체인 이전(권장) vs 평문 `accounts.json` 수용(상류 CLI들도 평문 저장)? — 보안 최대 레버.
2. **Copilot Win/Linux**: 완전 자동감지 패리티(키스토어 읽기) vs 수동 PAT 붙여넣기 허용? → 항목 6의 P0/P2를 가름.
3. **UI 방향**: A Cockpit(추천) / B Ledger / C Almanac, 그리고 다크·라이트·양쪽, "잔여 헤드룸 vs 소비량" 프레임, 브랜드 제약 유무.
4. **패키징 신원**(비용·리드타임): Apple Developer 계정 보유 여부 / Windows = Azure Trusted Signing(~$10월) vs OV 인증서 / macOS = Apple Silicon만 vs universal / Linux = AppImage만 vs deb·rpm 포함.
5. **자동 업데이트**: v1에 인앱 자동업데이트 도입 vs GitHub Releases 수동 다운로드(권장: 수동 우선, 업데이터는 P2).
6. **IPC 계약 가드**: `ts-rs` codegen(강함, `types.ts` 저작방식 변경) vs golden-JSON shape 테스트(무의존, 저렴)?
7. **HTTP 테스트 커버리지**: 6개 provider의 base URL을 env 주입 가능하게 하는 소규모 리팩토링 승인 여부.
8. **i18n**: 로케일(en/ko만?) · 기본 언어 · Rust `ProviderError`에 에러 코드 추가 의향(클린 픽스).
9. **메뉴바 전용 행동 확정**: Dock 아이콘 없음(Accessory) + single-instance가 의도된 UX인가.
10. **행동 변경 정리 승인**: 가짜 activity 피드 / 미연결 내비 / 죽은 `sort_index`·reorder 제거 OK 여부(또는 reorder UI를 실제 구현할지).
11. **카운트다운 정밀도**: 1초 vs 15~30초(항목 3 성능 픽스 범위를 가름).

---

## 6. 부록 — 에이전트 재개

각 영역 상세 리포트는 본 세션 대화에 전문 보존. 후속 심화는 아래 agentId로 재개 가능:
- 보안 `a1d00fd5d9ef3b4da` · 크로스-OS `a3b04b3f81ee9e664` · 리팩토링 `a251caf102a3339cc` · UI/UX `ac52892794af9d77b` · 테스트·CI `ab895261c17d495d6` · 패키징 `a9c6db79fa6d34dd5` · 성능 `a4ed9adcee9bb08ce` · 접근성·i18n `aec6fd7c15ba4ea94`
