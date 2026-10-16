# 개발 안내

[English](DEVELOPMENT.md) · **한국어**

## 빌드

필요한 것: Rust(GNU 툴체인) + MinGW-w64.

```powershell
winget install Rustlang.Rustup
rustup default stable-x86_64-pc-windows-gnu
winget install BrechtSanders.WinLibs.POSIX.MSVCRT
```

빌드하면 `dist\TB-Notepad.exe`가 만들어집니다. 앱이 켜져 있어 실행 파일이 잠겨 있으면, 예전 파일을 옆으로 비켜 두고 새 파일을 넣습니다.

```powershell
powershell -ExecutionPolicy Bypass -File .\build.ps1
```

테스트: `cargo test`

- 프로젝트 경로에 작은따옴표(예: `TB's Notepad`)가 있으면 MinGW 링커가 깨집니다. 이때 `build.ps1`은 `%LOCALAPPDATA%\tb-notepad\target`에 빌드하고, 그런 폴더에서 `cargo`를 직접 쓸 때는 `CARGO_TARGET_DIR`을 같은 곳으로 지정하세요 (또는 저장소에 올리지 않는 `.cargo/config.toml`에 `target-dir`).
- 로고는 `python scripts/make-logo.py`로 다시 만들 수 있습니다(`assets/logo.png`, `assets/logo.ico`). 실행 파일 아이콘은 `build.rs`가 windres로 `assets/app.rc`를 넣습니다.
- 실행 파일은 Windows에 기본으로 들어 있는 DLL만 써서, 다른 PC에 exe 하나만 복사해도 실행됩니다.

## 설정과 데이터 위치

- 설정과 세션: `%APPDATA%\tb-notepad\data\app.ron`
- 실행 파일 옆에 `portable.txt`를 만들면 실행 파일 옆 `data` 폴더에 저장합니다 (USB 휴대용).
- 환경 변수 `TB_NOTEPAD_DATA`로 저장 폴더를 지정하면 기본 메모 폴더도 그 안에 만듭니다 (테스트용).

## 성능 측정

환경 변수 `TB_NOTEPAD_PERF=<결과 파일>` 로 켜면 프레임 시간(과 문서 배치 · 꾸밈 그리기 · 검색 시간)을 모았다가 끝날 때 p50/p95/최대와 메모리를 씁니다.
`TB_NOTEPAD_PERF_SCRIPT=1` 이면 앱이 내장 입력 각본(끝으로 이동 → 40글자 입력 → Enter 10번 → 맨 위로 → 스크롤 40번 → 닫기)을
egui 에 직접 넣어 실행하므로 실제 키보드 · 마우스 입력이 필요 없습니다. `TB_NOTEPAD_PERF_SCRIPT=search` 는 사이드바 검색 각본입니다.

```powershell
python scripts/make-perf-data.py perf_data      # 100KB / 500KB / 950KB / 3MB 문서 + 메모 3000개
$env:TB_NOTEPAD_DATA = "$PWD\perf_data"; $env:TB_NOTEPAD_PERF = "$PWD\perf.txt"; $env:TB_NOTEPAD_PERF_SCRIPT = "1"
dist\TB-Notepad.exe "perf_data\TB Notes\큰 문서ŀKB.md"; Get-Content perf.txt
```

1MB 가 넘는 문서는 반응을 지키려고 문서 모드 서식 없이 보여 줍니다.

## Ctrl+V 그림 붙여넣기의 원리

egui는 클립보드에 텍스트가 없으면 Ctrl+V를 조용히 삼킵니다. 그래서 Windows에서는 창을 서브클래싱해
winit보다 먼저 키 입력을 보고, "텍스트는 없고 그림/파일이 있는" 경우에만 그림 붙여넣기를 실행합니다 (`src/paste_hook.rs`).

## 문서 모드의 원리

글자 자체는 그대로 두고(그래야 TextEdit이 그대로 편집할 수 있음) 모양만 바꿉니다 (`src/live.rs`).
`**`, `#` 같은 기호는 거의 폭이 없는 투명한 글자로 숨기고, 목록 점 · 체크박스 · 인용 막대 · 표 · 그림 · 소리/영상 카드는
배치된 글자 위치에 맞춰 따로 그립니다 (`src/app/decorations.rs`). 그림·표처럼 높이가 필요한 줄은 숨긴 글자의 줄 높이로 자리를 잡습니다.

## 화면 언어 (한국어 · English · 中文 · 日本語)

코드에는 한국어 문구를 그대로 쓰고 `t!("저장")` / `tf!("{} 저장됨", name)` 으로 감쌉니다 (`src/i18n.rs`).
번역은 `scripts/i18n_strings.py` 에 있고, 고친 뒤 `python scripts/gen-i18n.py` 를 실행하면 `src/i18n_table.rs` 를 다시 만들고
번역이 빠진 문구나 `{}` 개수가 다른 번역을 알려 줍니다. 언어는 설정 › 편집기에서 고르며, "자동"이면 Windows 화면 언어를 따릅니다.
처음 실행할 때 만드는 안내 메모는 `samples/welcome/<언어>.md` 입니다.

## 릴리스

버전 태그를 올리면 (`git tag -a v0.3.0 -m ... && git push origin v0.3.0`) `.github/workflows/release.yml` 이
GitHub Actions 에서 `TB-Notepad.exe` 를 빌드해 그 릴리스에 붙입니다. 릴리스가 아직 없으면 초안으로 만들어 둡니다.
저장소가 공개 상태면 빌드 출처 증명(attestation)도 남기므로, 받은 exe 가 이 소스로 이 워크플로에서 빌드된 것인지
누구나 확인할 수 있습니다:

```
gh attestation verify TB-Notepad.exe -R tbvwv123/TB-s-Notepad
```

먼저 `Cargo.toml` 과 `assets/app.rc` 의 버전을 올리세요. `.github/workflows/ci.yml` 은 푸시할 때마다 fmt · clippy · 테스트 · 번역 확인을 돌립니다.

## 코드 구조

| 파일 | 역할 |
| --- | --- |
| `src/i18n.rs` · `src/i18n_table.rs` | 화면 언어, `t!` / `tf!` 번역 매크로, 번역표 (`scripts/gen-i18n.py` 가 만듦) |
| `src/main.rs` | 진입점, 창 설정, 창 아이콘(`assets/logo.png`), 저장 폴더 결정 |
| `build.rs` | 실행 파일에 아이콘·파일 정보 넣기 (`assets/app.rc`, windres) |
| `scripts/make-logo.py` | 로고 만들기: 검정 타일 + 흰 TB + 빨간 커서 (`assets/`) |
| `src/app/mod.rs` | 앱 상태와 초기화, 화면 배치(패널 순서), 테마·글꼴 적용, 세션 저장 |
| `src/app/editor.rs` | 본문 편집기: 글자 배치(문서 모드/원본 보기), 그림 줄 커서 처리, 찾은 위치·줄 번호, 가짜 굵게 |
| `src/app/decorations.rs` | 문서 모드 꾸밈 그리기: 목록 점, 체크박스, 인용 막대, 표, 그림 선택·크기 손잡이, 링크 |
| `src/app/files.rs` | 열기·닫기·저장, 자동 저장(실패 알림·다시 시도), 밖에서 바뀐 파일 다시 읽기, 시작 메모 |
| `src/app/file_ops.rs` | 사이드바 파일 작업: 이름 바꾸기, 옮기기(링크·버전 기록이 따라감), 지우기 |
| `src/app/images.rs` | 그림·소리·영상·파일 링크 넣기·붙여넣기, 그림 크기·위치·삭제, 그림 앞/뒤 입력 처리 |
| `src/app/edit_menu.rs` | 본문 오른쪽 클릭 메뉴 (잘라내기·복사·붙여넣기·모두 선택·하이퍼링크 복사·링크 만들기·메모 링크 넣기), 오른쪽 클릭에도 선택 유지 |
| `src/emoji.rs` · `src/app/emoji_ui.rs` | 커스텀 이모지: `.emoji` 폴더, 이름 규칙, 글 속 문법 / 넣기, 등록, 한 덩어리로 지우기 |
| `src/app/slash.rs` | `/` 명령 목록: 명령 표, 걸러내기(초성 포함), 열기·닫기·키 처리, 실행 |
| `src/app/note_links.rs` | 링크 따라가기(메모 열고 줄·제목으로, 폴더는 사이드바, 그 밖은 탐색기), 이 줄 링크 복사, 메모 링크 고르기 창 |
| `src/app/media_cards.rs` | 소리 카드(재생/멈춤·재생 막대·소리 크기)와 영상 카드(첫 장면·기본 플레이어로 열기), 파일 정보 읽기 |
| `src/app/find.rs` | 찾기 / 바꾸기 카드 |
| `src/app/chrome.rs` | ⋯ 메뉴, 사이드바 머리, 상단 바, 서식 도구, 상태 표시줄 |
| `src/app/input.rs` | 단축키, 끌어다 놓기, 창 닫기 |
| `src/app/dialogs.rs` | 대화상자 (저장 확인, 휴지통, 새 폴더, 밖에서 바뀐 파일) |
| `src/app/settings_ui.rs` | 설정 창 |
| `src/editing.rs` | 입력 도우미(순수 함수): 목록 이어 쓰기(Enter), 들여쓰기(Tab), 토글(`<details>`) |
| `src/history.rs` | 이전 버전: 저장 직전 기록(5분마다 · 많이 지웠을 때), 목록, 되돌리기 |
| `src/app/typing.rs` | 입력 도우미를 편집기에 연결 (Enter, Tab, Ctrl+Enter, `>> `) |
| `src/app/history_ui.rs` | 이전 버전 창 (목록, 미리보기, 되돌리기) |
| `src/perf.rs` | 개발용 성능 기록과 내장 입력 각본 (`TB_NOTEPAD_PERF`) |
| `src/media.rs` | 소리·영상 종류 판별, 소리 재생기(rodio), Windows 셸 썸네일, 로컬 파일 링크 해석 |
| `src/links.rs` | 메모 사이 링크 풀기(경로 · `#L12` 줄 · `#제목`), 제목 비교용 slug, 상대 경로 만들기, 붙여넣은 링크를 상대 경로로 |
| `src/live.rs` | 문서 모드 배치: 기호 숨기기, 서식 적용, 목록 점·체크박스·표·그림 등 꾸밈 정보 |
| `src/sidebar.rs` | 폴더 트리, 검색, 이름 바꾸기, 휴지통 삭제, 기본 메모 폴더 |
| `src/document.rs` | 문서 상태, 열기·저장, 밖에서 바뀐 것 알아채기 |
| `src/encoding.rs` | 텍스트 인코딩: BOM · UTF-8 · 옛 인코딩 감지(chardetng)와 변환 |
| `src/safe_write.rs` | 안전한 파일 바꿔 쓰기(임시 파일 → 교체, 교체가 막히면 직접 저장만 백업해 두고 제자리 쓰기, 자동 저장은 원본을 두고 다시 시도) |
| `src/markdown.rs` | 원본 보기 문법 강조, HTML 내보내기, 서식 도구 동작, 찾기 |
| `src/settings.rs` | 설정 값, 테마 팔레트와 전역 스타일 |
| `src/fonts.rs` | 시스템 글꼴 목록, 본문/굵게/코드 글꼴 묶음, 굵은 변형 찾기·가변 글꼴 굵기·가짜 굵게, 한글·영문 기준선 보정 |
| `src/ui_kit.rs` | 직접 그리는 벡터 아이콘, 납작한 버튼, 스위치 |
| `src/paste_hook.rs` | Windows 창 서브클래싱으로 Ctrl+V 그림 붙여넣기 감지 |
| `src/mcp.rs` | `--mcp` 모드: AI용 로컬 MCP 서버 (stdio JSON-RPC, 메모 폴더 경로 제한, 메모 도구) — [MCP.ko.md](MCP.ko.md) |
