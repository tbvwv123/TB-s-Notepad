[English](MCP.md) · [한국어](MCP.ko.md) · [简体中文](MCP.zh-CN.md) · **日本語**

# AI といっしょに使う (MCP)

Claude Desktop や Claude Code などの AI アプリが、メモフォルダーを直接読み書きできます。
**クラウドサーバーもログインも不要です。** AI アプリがこの PC で `TB-Notepad.exe --mcp` を直接実行し、
標準入出力 (stdio) で [MCP](https://modelcontextprotocol.io) のメッセージをやり取りします。ウィンドウは開かず、インターネットにも接続しません。
それぞれが自分の PC で自分のメモだけを扱います。

## AI が使えるツール

| ツール | 内容 |
| --- | --- |
| `list_notes` | メモフォルダーのサブフォルダーとメモをツリーで表示 (`folder` で範囲を絞り込み) |
| `read_note` | メモを 1 つ読む |
| `search_notes` | ファイル名と内容から検索 (大文字小文字を区別せず、一致した行も表示) |
| `create_note` | 新しいメモを作成 (ないサブフォルダーも作成、同じ名前があれば失敗) |
| `update_note` | メモの内容全体を置き換え |
| `append_to_note` | メモの末尾に追記 |

- 対象はアプリ設定の **メモフォルダー** (サイドバーのフォルダー) です。アプリでフォルダーを追加すると、次のリクエストから見えます。
- パスは `TB Notes/日記/10月.md` のようにメモフォルダー名から書きます (メモフォルダーが 1 つだけなら先頭の名前は省略可)。
- メモフォルダーの外には出られません: `..`、絶対パス、ドライブ文字、外を指すリンク (ジャンクション) はすべて拒否します。
- `.md` と `.txt` だけを扱います。削除・名前変更のツールはありません。
- 保存はアプリと同じく一時ファイルに書いてから置き換え、元のエンコード (UTF-8、UTF-16、CP949、Shift-JIS など) と改行コード (CRLF/LF) を保ちます。
- 設定の保存場所のルール (`TB_NOTEPAD_DATA`、ポータブルモード) もアプリと同じです。ポータブルで使う場合はその実行ファイルのパスを登録してください。
- AI に見せるツールの説明と結果メッセージは英語です (AI はどの言語で尋ねても理解します)。

## Claude Desktop に接続

`%APPDATA%\Claude\claude_desktop_config.json` (Claude Desktop の 設定 › 開発者 › 構成を編集) に追加して Claude Desktop を再起動します。
`command` は実行ファイルの実際のパスに変えてください (JSON では `\` を 2 つ書きます)。

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

> Microsoft Store 版の Claude Desktop は
> `%LOCALAPPDATA%\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\Claude\claude_desktop_config.json` から設定を読みます。
> 変更したあとはトレイアイコンから **完全に終了** して起動し直すと反映されます。

## Claude Code に接続

```powershell
claude mcp add --scope user tb-notepad -- "C:\Apps\TB-Notepad.exe" --mcp
```

`--scope user` はすべてのプロジェクトで使うという意味です。確認は `claude mcp list`。

## アプリを開いているとき

アプリはウィンドウがフォーカスを得たときと 2 秒ごとに、開いているメモが外部で変わったかを確認します。

- アプリで編集していないメモは、AI の変更を **静かに再読み込み** します (カーソル位置はそのまま)。
- アプリで編集中 (未保存) のメモを AI が変えた場合は、上書きしないよう **自動保存を止め**、
  「ファイルが外部で変更されました」ダイアログで *再読み込み* / *自分の内容を残す* を選べます。

人と AI が同じメモを同時に編集するとどちらかを選ぶことになるので、AI に任せたメモはしばらく触らないのがおすすめです。

## 動作確認

```powershell
python -X utf8 scripts\mcp-smoke.py dist\TB-Notepad.exe
```

一時フォルダーで `initialize` → `tools/list` → `tools/call` (作成、追記、一覧、検索、読み取り、外部アクセスの拒否) を送り、応答を表示します。

## 実装メモ

公式の Rust SDK (`rmcp`) は tokio の非同期ランタイムや JSON スキーマ生成 (schemars) なども一緒に取り込みます。
ここで必要なのは 1 行ずつやり取りする JSON-RPC 数種類 (`initialize`、`ping`、`tools/list`、`tools/call`) だけなので、
`src/mcp.rs` に直接実装しました。増えた依存は `serde_json` だけで (`ron` は eframe がすでに使っていたもの)、実行ファイルのサイズも変わりません。
