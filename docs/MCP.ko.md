[English](MCP.md) · **한국어** · [简体中文](MCP.zh-CN.md) · [日本語](MCP.ja.md)

# AI와 함께 쓰기 (MCP)

Claude Desktop이나 Claude Code 같은 AI가 메모 폴더를 직접 읽고 쓸 수 있습니다.
**클라우드 서버도, 로그인도 필요 없습니다.** AI 프로그램이 이 PC에서 `TB-Notepad.exe --mcp`를 직접 실행하고,
표준 입출력(stdio)으로 [MCP](https://modelcontextprotocol.io) 메시지를 주고받습니다. 창은 뜨지 않고, 인터넷으로 나가는 것도 없습니다.
각자 자기 PC에서 자기 메모 폴더만 다룹니다.

## AI가 쓸 수 있는 도구

| 도구 | 하는 일 |
| --- | --- |
| `list_notes` | 메모 폴더의 하위 폴더와 메모를 트리로 보기 (`folder`로 범위 좁히기) |
| `read_note` | 메모 하나 읽기 |
| `search_notes` | 파일 이름과 내용에서 찾기 (대소문자 무시, 맞는 줄 함께) |
| `create_note` | 새 메모 만들기 (없는 하위 폴더도 만듦, 같은 이름이 있으면 실패) |
| `update_note` | 메모 내용 전체 바꾸기 |
| `append_to_note` | 메모 끝에 덧붙이기 |

- 다루는 범위는 앱 설정의 **메모 폴더**(사이드바의 폴더들)입니다. 앱에서 폴더를 더하면 다음 요청부터 바로 보입니다.
- 경로는 `TB Notes/일기/10월.md`처럼 메모 폴더 이름부터 씁니다 (메모 폴더가 하나뿐이면 앞의 이름은 빼도 됨).
- 메모 폴더 밖으로는 나갈 수 없습니다: `..`, 절대 경로, 드라이브 문자, 폴더 밖을 가리키는 링크(정션)는 모두 거부합니다.
- `.md`와 `.txt`만 다룹니다. 지우기·이름 바꾸기 도구는 없습니다.
- 저장은 앱과 똑같이 임시 파일에 쓴 뒤 교체하고, 원래 인코딩(UTF-8, UTF-16, CP949 등)과 줄 끝(CRLF/LF)을 유지합니다.
- 설정 위치 규칙(`TB_NOTEPAD_DATA`, 포터블 모드)도 앱과 같습니다. 포터블로 쓰면 그 실행 파일 경로를 등록하세요.
- AI에게 보여 주는 도구 설명과 결과 메시지는 영어입니다 (AI는 어느 언어로 물어봐도 알아듣습니다).

## Claude Desktop에 연결

`%APPDATA%\Claude\claude_desktop_config.json` (Claude Desktop의 설정 › 개발자 › 설정 편집)에 추가하고 Claude Desktop을 다시 켭니다.
`command`는 실행 파일이 있는 실제 경로로 바꾸세요 (JSON에서는 `\`를 두 번 씁니다).

```json
{
  "mcpServers": {
    "tb-notepad": {
      "command": "C:\\Apps\\TB-Notepad.exe",
      "args": ["--mcp"]
    }
  }
}
```

> Microsoft Store판 Claude Desktop은 설정 파일을
> `%LOCALAPPDATA%\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\Claude\claude_desktop_config.json` 에서 읽습니다.
> 설정을 바꾼 뒤에는 트레이 아이콘에서 **완전히 종료**하고 다시 켜야 적용됩니다.

## Claude Code에 연결

```powershell
claude mcp add --scope user tb-notepad -- "C:\Apps\TB-Notepad.exe" --mcp
```

`--scope user`는 모든 프로젝트에서 쓰겠다는 뜻입니다. 확인은 `claude mcp list`.

## 앱이 켜져 있을 때

앱은 창이 포커스를 얻을 때와 2초마다 열린 메모가 밖에서 바뀌었는지 확인합니다.

- 앱에서 고치지 않은 메모는 AI가 바꾼 내용을 **조용히 다시 불러옵니다** (커서 위치 유지).
- 앱에서 고치는 중(저장 전)이던 메모를 AI가 바꾸면, 자동 저장이 덮어쓰지 않도록 **자동 저장을 멈추고**
  "파일이 밖에서 바뀌었습니다" 창에서 *다시 불러오기* / *내 내용 유지* 중 고르게 합니다.

그래도 같은 메모를 사람과 AI가 동시에 고치면 한쪽을 골라야 하니, AI에게 맡긴 메모는 잠시 손대지 않는 것이 편합니다.

## 동작 확인

```powershell
python -X utf8 scripts\mcp-smoke.py dist\TB-Notepad.exe
```

임시 폴더에서 `initialize` → `tools/list` → `tools/call`(만들기, 덧붙이기, 목록, 검색, 읽기, 폴더 밖 접근 거부)을 보내고 응답을 보여 줍니다.

## 구현 메모

공식 Rust SDK(`rmcp`)는 tokio 비동기 런타임, JSON 스키마 생성(schemars) 등을 함께 끌고 옵니다.
여기서 필요한 것은 한 줄에 하나씩 오가는 JSON-RPC 몇 가지(`initialize`, `ping`, `tools/list`, `tools/call`)뿐이라
`src/mcp.rs`에 직접 구현했습니다. 새로 늘어난 의존성은 `serde_json` 하나이고 (`ron`은 eframe이 이미 쓰던 것), 실행 파일 크기도 그대로입니다.
