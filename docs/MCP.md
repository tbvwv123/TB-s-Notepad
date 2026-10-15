**English** · [한국어](MCP.ko.md) · [简体中文](MCP.zh-CN.md) · [日本語](MCP.ja.md)

# Working with AI (MCP)

AI apps like Claude Desktop or Claude Code can read and write your notes folders directly.
**No cloud server and no login.** The AI app runs `TB-Notepad.exe --mcp` on this PC and talks
[MCP](https://modelcontextprotocol.io) over standard input/output (stdio). No window opens and nothing goes out to the internet.
Everyone works only with their own notes on their own PC.

## Tools the AI can use

| Tool | What it does |
| --- | --- |
| `list_notes` | Shows the subfolders and notes in the notes folders as a tree (narrow it with `folder`) |
| `read_note` | Reads one note |
| `search_notes` | Searches file names and contents (case-insensitive, with matching lines) |
| `create_note` | Creates a new note (creates missing subfolders; fails if the name is taken) |
| `update_note` | Replaces a note's whole content |
| `append_to_note` | Appends to the end of a note |

- The scope is the app's **notes folders** (the folders in the sidebar). Folders you add in the app show up on the next request.
- Paths start with the notes folder name, like `TB Notes/Journal/October.md` (with a single notes folder, the name can be left out).
- Nothing outside the notes folders is reachable: `..`, absolute paths, drive letters and links (junctions) pointing outside are all rejected.
- Only `.md` and `.txt` files. There are no delete or rename tools.
- Saving works like the app: write to a temp file, then replace, keeping the original encoding (UTF-8, UTF-16, CP949…) and line endings (CRLF/LF).
- The settings location rules (`TB_NOTEPAD_DATA`, portable mode) match the app. If you use it portably, register that exe's path.

## Connect to Claude Desktop

Add this to `%APPDATA%\Claude\claude_desktop_config.json` (Claude Desktop › Settings › Developer › Edit config) and restart Claude Desktop.
Change `command` to where your exe actually is (backslashes are doubled in JSON).

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

> The Microsoft Store version of Claude Desktop reads its config from
> `%LOCALAPPDATA%\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\Claude\claude_desktop_config.json`.
> After changing it, **quit completely** from the tray icon and start it again.

## Connect to Claude Code

```powershell
claude mcp add --scope user tb-notepad -- "C:\Apps\TB-Notepad.exe" --mcp
```

`--scope user` makes it available in every project. Check with `claude mcp list`.

## While the app is open

The app checks whether open notes changed outside it whenever the window gets focus, and every 2 seconds.

- Notes you haven't edited in the app are **reloaded quietly** with the AI's changes (the cursor stays put).
- If the AI changes a note you're editing (not yet saved), **autosave pauses** so nothing is overwritten,
  and a "The file changed outside the app" dialog lets you choose *Reload* or *Keep mine*.

If a person and the AI edit the same note at the same time, one side has to win, so it's easiest to leave a note alone while the AI works on it.

## Smoke test

```powershell
python -X utf8 scripts\mcp-smoke.py dist\TB-Notepad.exe
```

Sends `initialize` → `tools/list` → `tools/call` (create, append, list, search, read, rejected outside access) in a temp folder and prints the responses.

## Implementation note

The official Rust SDK (`rmcp`) brings in the tokio async runtime, JSON schema generation (schemars) and more.
All that's needed here is a handful of line-delimited JSON-RPC messages (`initialize`, `ping`, `tools/list`, `tools/call`),
so it's implemented directly in `src/mcp.rs`. The only added dependency is `serde_json` (`ron` was already used by eframe), and the exe size didn't change.
