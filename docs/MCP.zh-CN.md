[English](MCP.md) · [한국어](MCP.ko.md) · **简体中文** · [日本語](MCP.ja.md)

# 与 AI 一起使用 (MCP)

Claude Desktop、Claude Code 等 AI 应用可以直接读写你的笔记文件夹。
**无需云服务器，也无需登录。** AI 应用会在这台电脑上直接运行 `TB-Notepad.exe --mcp`，
通过标准输入输出 (stdio) 收发 [MCP](https://modelcontextprotocol.io) 消息。不会打开窗口，也不会连接互联网。
每个人只在自己的电脑上处理自己的笔记。

## AI 可以使用的工具

| 工具 | 功能 |
| --- | --- |
| `list_notes` | 以树状显示笔记文件夹中的子文件夹和笔记（可用 `folder` 缩小范围） |
| `read_note` | 读取一篇笔记 |
| `search_notes` | 在文件名和内容中查找（不区分大小写，附带匹配的行） |
| `create_note` | 新建笔记（会创建缺少的子文件夹；同名文件已存在时失败） |
| `update_note` | 替换笔记的全部内容 |
| `append_to_note` | 在笔记末尾追加内容 |

- 范围是应用设置中的 **笔记文件夹**（侧边栏中的文件夹）。在应用中添加文件夹后，下一次请求即可看到。
- 路径以笔记文件夹名开头，如 `TB Notes/日记/10月.md`（只有一个笔记文件夹时可省略开头的名称）。
- 无法访问笔记文件夹以外的位置：`..`、绝对路径、盘符以及指向外部的链接（联接）都会被拒绝。
- 只处理 `.md` 和 `.txt`。没有删除和重命名工具。
- 保存方式与应用相同：先写入临时文件再替换，并保留原编码（UTF-8、UTF-16、CP949、GBK 等）和换行符 (CRLF/LF)。
- 设置位置规则（`TB_NOTEPAD_DATA`、便携模式）也与应用相同。便携使用时请注册该程序的路径。
- 给 AI 看的工具说明和结果消息是英文的（无论用哪种语言提问，AI 都能理解）。

## 连接到 Claude Desktop

把以下内容加入 `%APPDATA%\Claude\claude_desktop_config.json`（Claude Desktop › 设置 › 开发者 › 编辑配置），然后重启 Claude Desktop。
请把 `command` 改成程序的实际路径（JSON 中 `\` 要写两次）。

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

> Microsoft Store 版的 Claude Desktop 会从
> `%LOCALAPPDATA%\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\Claude\claude_desktop_config.json` 读取配置。
> 修改后需要从托盘图标 **完全退出** 再重新启动才会生效。

## 连接到 Claude Code

```powershell
claude mcp add --scope user tb-notepad -- "C:\Apps\TB-Notepad.exe" --mcp
```

`--scope user` 表示在所有项目中使用。可用 `claude mcp list` 确认。

## 应用打开时

应用会在窗口获得焦点时以及每 2 秒检查一次打开的笔记是否在外部被修改。

- 在应用中没有修改过的笔记，会 **自动安静地重新载入** AI 的修改（光标位置不变）。
- 如果 AI 修改了你正在编辑（尚未保存）的笔记，为避免覆盖会 **暂停自动保存**，
  并在“文件已在外部被修改”对话框中让你选择 *重新载入* 或 *保留我的*。

人和 AI 同时修改同一篇笔记时只能保留一边，所以交给 AI 处理的笔记最好暂时不要动。

## 运行测试

```powershell
python -X utf8 scripts\mcp-smoke.py dist\TB-Notepad.exe
```

在临时文件夹中发送 `initialize` → `tools/list` → `tools/call`（新建、追加、列表、搜索、读取、拒绝外部访问）并显示响应。

## 实现说明

官方 Rust SDK (`rmcp`) 会带入 tokio 异步运行时、JSON Schema 生成 (schemars) 等。
这里只需要几种按行收发的 JSON-RPC 消息（`initialize`、`ping`、`tools/list`、`tools/call`），
所以直接在 `src/mcp.rs` 中实现。新增的依赖只有 `serde_json`（`ron` 本来就被 eframe 使用），程序大小也没有变化。
