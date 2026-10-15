//! `--mcp`: 창 없이 stdio로 MCP(Model Context Protocol) 서버로 동작합니다.
//!
//! Claude Desktop, Claude Code 같은 AI 프로그램이 이 실행 파일을 직접 띄우고 표준 입출력으로
//! JSON-RPC 메시지를 한 줄에 하나씩 주고받습니다. 네트워크도 로그인도 없이, 이 PC의 메모 폴더
//! (설정의 `roots`)만 다룹니다.
//!
//! 경로는 항상 "메모 폴더 이름/하위 폴더/파일.md" 꼴입니다 (예: `TB Notes/일기/10월.md`).
//! 메모 폴더가 하나뿐이면 맨 앞의 폴더 이름은 빼도 됩니다.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Component, Path, PathBuf};

use serde_json::{Value, json};

use crate::document::Document;
use crate::safe_write::WhenBlocked;
use crate::settings::Settings;

/// 지원하는 MCP 버전 (새 것부터). 클라이언트가 이 중 하나를 요청하면 그대로 씁니다.
const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];
/// 다루는 파일 종류
const NOTE_EXTENSIONS: &[&str] = &["md", "txt"];
/// 이보다 큰 파일은 읽지 않습니다 (메모가 아닐 가능성이 큼)
const MAX_NOTE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_LIST_ENTRIES: usize = 2000;
const MAX_SEARCH_RESULTS: usize = 50;
const MAX_SNIPPETS: usize = 3;

const INSTRUCTIONS: &str = "Reads and writes the notes folders of TB's Notepad (.md/.txt files on this PC). \
Write paths exactly as list_notes shows them: 'notes folder name/subfolder/file.md'. \
Paths outside the notes folders, absolute paths and '..' are not allowed. New notes are usually .md (Markdown).";

/// stdin에서 메시지를 읽어 stdout으로 답합니다. stdin이 닫히면 끝납니다.
pub fn run() {
    let server = Server::default();
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = server.handle_line(&line)
            && writeln!(out, "{reply}").and_then(|_| out.flush()).is_err()
        {
            break;
        }
    }
}

/// JSON-RPC 처리. 메모 폴더는 요청마다 설정에서 다시 읽어, 앱에서 폴더를 더하면 바로 반영됩니다.
#[derive(Default)]
pub struct Server {
    /// 테스트용: 설정 대신 이 폴더들을 씁니다.
    roots_override: Option<Vec<PathBuf>>,
}

impl Server {
    pub fn handle_line(&self, line: &str) -> Option<Value> {
        match serde_json::from_str::<Value>(line) {
            Ok(Value::Array(batch)) => {
                let replies: Vec<Value> = batch.iter().filter_map(|m| self.handle(m)).collect();
                (!replies.is_empty()).then_some(Value::Array(replies))
            }
            Ok(msg) => self.handle(&msg),
            Err(e) => Some(rpc_error(Value::Null, -32700, &format!("Invalid JSON: {e}"))),
        }
    }

    /// 요청 하나를 처리합니다. 알림(`id` 없음)에는 답하지 않습니다.
    fn handle(&self, msg: &Value) -> Option<Value> {
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(Value::as_str);
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let (Some(id), Some(method)) = (id, method) else {
            // 알림이거나, 우리가 보낸 적 없는 요청에 대한 응답
            return None;
        };
        let result = match method {
            "initialize" => Ok(initialize_result(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tool_definitions() })),
            "tools/call" => self.call_tool(&params),
            _ => Err((-32601, format!("Unsupported method: {method}"))),
        };
        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => rpc_error(id, code, &message),
        })
    }

    fn call_tool(&self, params: &Value) -> Result<Value, (i64, String)> {
        let name = params.get("name").and_then(Value::as_str).ok_or((-32602, "Missing tool name (name)".to_owned()))?;
        let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
        let vault = self.vault();
        let outcome = match name {
            "list_notes" => vault.list_notes(opt_str(&args, "folder")),
            "read_note" => req_str(&args, "path").and_then(|p| vault.read_note(p)),
            "search_notes" => req_str(&args, "query").and_then(|q| vault.search_notes(q, opt_str(&args, "folder"))),
            "create_note" => {
                req_str(&args, "path").and_then(|p| vault.create_note(p, opt_str(&args, "content").unwrap_or("")))
            }
            "update_note" => {
                req_str(&args, "path").and_then(|p| req_str(&args, "content").and_then(|c| vault.update_note(p, c)))
            }
            "append_to_note" => {
                req_str(&args, "path").and_then(|p| req_str(&args, "content").and_then(|c| vault.append_to_note(p, c)))
            }
            _ => return Err((-32602, format!("Unknown tool: {name}"))),
        };
        // 도구 실행 중 생긴 문제는 JSON-RPC 오류가 아니라 결과의 isError로 알려서 AI가 읽고 고칠 수 있게 합니다.
        let (text, is_error) = match outcome {
            Ok(text) => (text, false),
            Err(text) => (text, true),
        };
        Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
    }

    fn vault(&self) -> Vault {
        match &self.roots_override {
            Some(roots) => Vault::new(roots, "images"),
            None => Vault::from_settings(),
        }
    }
}

fn initialize_result(params: &Value) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    let version = requested.filter(|v| PROTOCOL_VERSIONS.contains(v)).unwrap_or(PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "tb-notepad", "title": "TB's Notepad", "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn opt_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn req_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    opt_str(args, key).ok_or_else(|| format!("'{key}' (string) is required"))
}

fn tool_definitions() -> Value {
    let path = json!({ "type": "string", "description": "Note path, e.g. 'TB Notes/Journal/October.md' (exactly as list_notes shows it)" });
    let folder = json!({ "type": "string", "description": "Only look inside this folder, e.g. 'TB Notes/Journal'. Leave empty for all notes folders" });
    let content = json!({ "type": "string", "description": "Note content (Markdown)" });
    json!([
        {
            "name": "list_notes",
            "title": "List notes",
            "description": "Shows the subfolders and notes (.md/.txt) in the notes folders as a tree. Use each line's path as-is with the other tools.",
            "inputSchema": { "type": "object", "properties": { "folder": folder } },
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "read_note",
            "title": "Read note",
            "description": "Reads the content of one note.",
            "inputSchema": { "type": "object", "properties": { "path": path }, "required": ["path"] },
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "search_notes",
            "title": "Search notes",
            "description": "Finds text in file names and contents (case-insensitive). Shows matching notes' paths and lines.",
            "inputSchema": {
                "type": "object",
                "properties": { "query": { "type": "string", "description": "Text to find" }, "folder": folder },
                "required": ["query"],
            },
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "create_note",
            "title": "Create note",
            "description": "Creates a new note file, including any missing subfolders. Fails if a file with the same name exists.",
            "inputSchema": { "type": "object", "properties": { "path": path, "content": content }, "required": ["path"] },
            "annotations": { "readOnlyHint": false, "destructiveHint": false },
        },
        {
            "name": "update_note",
            "title": "Update note",
            "description": "Replaces the whole content of an existing note. Keeps its original encoding and line endings (CRLF/LF).",
            "inputSchema": { "type": "object", "properties": { "path": path, "content": content }, "required": ["path", "content"] },
            "annotations": { "readOnlyHint": false, "destructiveHint": true, "idempotentHint": true },
        },
        {
            "name": "append_to_note",
            "title": "Append to note",
            "description": "Appends content to the end of an existing note, adding a line break first if the last line doesn't end with one.",
            "inputSchema": { "type": "object", "properties": { "path": path, "content": content }, "required": ["path", "content"] },
            "annotations": { "readOnlyHint": false, "destructiveHint": false },
        },
    ])
}

// ---------------------------------------------------------------------------
// 메모 폴더와 경로 제한

struct Root {
    /// 경로 맨 앞에 쓰는 이름 (폴더 이름, 겹치면 " (2)" 등을 붙임)
    name: String,
    dir: PathBuf,
    /// 심볼릭 링크 등을 모두 푼 실제 경로. 모든 접근은 이 안이어야 합니다.
    canon: PathBuf,
}

pub struct Vault {
    roots: Vec<Root>,
    /// 붙여넣은 그림을 두는 폴더 이름 (목록에서 뺌)
    image_folder: String,
}

/// 경로를 풀어 낸 결과
struct Target<'a> {
    root: &'a Root,
    path: PathBuf,
}

impl Vault {
    /// 앱 설정(`app.ron`)의 메모 폴더. 설정이 없으면 기본 메모 폴더(문서\TB Notes).
    fn from_settings() -> Self {
        let settings: Settings = std::fs::read_to_string(crate::data_dir().join("app.ron"))
            .ok()
            .and_then(|s| ron::from_str::<HashMap<String, String>>(&s).ok())
            .and_then(|map| ron::from_str(map.get("settings")?).ok())
            .unwrap_or_default();
        let mut roots = settings.roots;
        if roots.is_empty() {
            let dir = crate::sidebar::default_notes_dir();
            let _ = std::fs::create_dir_all(&dir);
            roots.push(dir);
        }
        Self::new(&roots, &settings.image_folder)
    }

    fn new(dirs: &[PathBuf], image_folder: &str) -> Self {
        let mut roots: Vec<Root> = Vec::new();
        for dir in dirs {
            // 없어진 폴더는 건너뜁니다.
            let Ok(canon) = dir.canonicalize() else { continue };
            if !canon.is_dir() || roots.iter().any(|r| r.canon == canon) {
                continue;
            }
            let base = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Notes".to_owned());
            let mut name = base.clone();
            let mut n = 2;
            while roots.iter().any(|r| r.name.eq_ignore_ascii_case(&name)) {
                name = format!("{base} ({n})");
                n += 1;
            }
            roots.push(Root { name, dir: dir.clone(), canon });
        }
        Self { roots, image_folder: image_folder.to_owned() }
    }

    /// AI가 준 경로를 메모 폴더 안의 실제 경로로 바꿉니다. 메모 폴더 밖으로 나가는 경로는 거부합니다.
    fn resolve(&self, input: &str) -> Result<Target<'_>, String> {
        if self.roots.is_empty() {
            return Err("There are no notes folders. Add one in TB's Notepad first.".to_owned());
        }
        let input = input.trim().replace('\\', "/");
        if input.starts_with('/') || Path::new(&input).has_root() {
            return Err(format!("Absolute paths are not allowed: {input}"));
        }
        let mut parts: Vec<&str> = Vec::new();
        for part in input.split('/') {
            match part {
                "" | "." => {}
                ".." => return Err("'..' is not allowed. Only paths inside the notes folders work.".to_owned()),
                _ => {
                    check_name(part)?;
                    parts.push(part);
                }
            }
        }
        // 맨 앞이 메모 폴더 이름이면 그 폴더, 폴더가 하나뿐이면 이름을 빼도 됩니다.
        let (root, rest) =
            match parts.first().and_then(|first| self.roots.iter().find(|r| r.name.eq_ignore_ascii_case(first))) {
                Some(root) => (root, &parts[1..]),
                None if self.roots.len() == 1 => (&self.roots[0], &parts[..]),
                None => {
                    let names: Vec<&str> = self.roots.iter().map(|r| r.name.as_str()).collect();
                    return Err(format!("Paths must start with a notes folder name: {}", names.join(", ")));
                }
            };
        let mut path = root.dir.clone();
        for part in rest {
            path.push(part);
        }
        // 글자로는 안전해도 폴더 안의 링크(정션)가 밖을 가리킬 수 있으므로 실제 위치를 확인합니다.
        if !within(&path, &root.canon) {
            return Err("This path points outside the notes folders.".to_owned());
        }
        Ok(Target { root, path })
    }

    /// 메모 파일 경로 (.md / .txt)
    fn resolve_note(&self, input: &str) -> Result<Target<'_>, String> {
        let target = self.resolve(input)?;
        if !is_note(&target.path) {
            return Err(format!("Only note files (.md, .txt) are supported: {input}"));
        }
        Ok(target)
    }

    /// 사용자에게 보여 줄 경로: "메모 폴더 이름/하위/파일.md"
    fn display(&self, root: &Root, path: &Path) -> String {
        let mut s = root.name.clone();
        if let Ok(rel) = path.strip_prefix(&root.dir) {
            for c in rel.components() {
                s.push('/');
                s.push_str(&c.as_os_str().to_string_lossy());
            }
        }
        s
    }

    /// 살펴볼 폴더들: `folder`가 있으면 그 폴더 하나, 없으면 모든 메모 폴더
    fn scopes(&self, folder: Option<&str>) -> Result<Vec<Target<'_>>, String> {
        match folder.map(str::trim).filter(|f| !f.is_empty()) {
            Some(folder) => {
                let target = self.resolve(folder)?;
                if !target.path.is_dir() {
                    return Err(format!("No such folder: {folder}"));
                }
                Ok(vec![target])
            }
            None if self.roots.is_empty() => {
                Err("There are no notes folders. Add one in TB's Notepad first.".to_owned())
            }
            None => Ok(self.roots.iter().map(|root| Target { root, path: root.dir.clone() }).collect()),
        }
    }

    // -----------------------------------------------------------------------
    // 도구

    fn list_notes(&self, folder: Option<&str>) -> Result<String, String> {
        let mut out = String::new();
        let mut count = 0;
        for scope in self.scopes(folder)? {
            out.push_str(&self.display(scope.root, &scope.path));
            out.push_str("/\n");
            self.tree(scope.root, &scope.path, 1, &mut out, &mut count);
        }
        if count >= MAX_LIST_ENTRIES {
            out.push_str(&format!(
                "… Too many entries; showing the first {MAX_LIST_ENTRIES}. Narrow it down with folder.\n"
            ));
        }
        Ok(out)
    }

    fn tree(&self, root: &Root, dir: &Path, depth: usize, out: &mut String, count: &mut usize) {
        if depth > 8 {
            return;
        }
        for (path, is_dir) in self.entries(dir) {
            if *count >= MAX_LIST_ENTRIES {
                return;
            }
            *count += 1;
            out.push_str(&"  ".repeat(depth));
            out.push_str(&self.display(root, &path));
            if is_dir {
                out.push_str("/\n");
                self.tree(root, &path, depth + 1, out, count);
            } else {
                out.push('\n');
            }
        }
    }

    /// 폴더 안의 하위 폴더와 메모 (숨김 파일, 링크, 그림 폴더 등은 뺌). 폴더 먼저, 이름순.
    fn entries(&self, dir: &Path) -> Vec<(PathBuf, bool)> {
        let mut out = Vec::new();
        let Ok(rd) = std::fs::read_dir(dir) else { return out };
        for entry in rd.flatten().take(3000) {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(ft) = entry.file_type() else { continue };
            // 링크·정션은 메모 폴더 밖을 가리킬 수 있으므로 따라가지 않습니다.
            if name.starts_with('.') || name.starts_with('~') || ft.is_symlink() {
                continue;
            }
            let path = entry.path();
            if ft.is_dir() {
                if crate::sidebar::SKIP_DIRS.iter().any(|s| s.eq_ignore_ascii_case(&name))
                    || name.eq_ignore_ascii_case(&self.image_folder)
                {
                    continue;
                }
                out.push((path, true));
            } else if is_note(&path) {
                out.push((path, false));
            }
        }
        out.sort_by(|a, b| {
            b.1.cmp(&a.1).then_with(|| {
                crate::sidebar::natural_cmp(
                    &a.0.file_name().unwrap_or_default().to_string_lossy(),
                    &b.0.file_name().unwrap_or_default().to_string_lossy(),
                )
            })
        });
        out
    }

    fn all_notes(&self, dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        if depth > 8 {
            return;
        }
        for (path, is_dir) in self.entries(dir) {
            if is_dir {
                self.all_notes(&path, depth + 1, out);
            } else {
                out.push(path);
            }
        }
    }

    fn read_note(&self, input: &str) -> Result<String, String> {
        let target = self.resolve_note(input)?;
        Ok(open_existing(&target.path, input)?.text)
    }

    fn search_notes(&self, query: &str, folder: Option<&str>) -> Result<String, String> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Err("The search text (query) is empty.".to_owned());
        }
        let mut out = String::new();
        let mut found = 0;
        'scopes: for scope in self.scopes(folder)? {
            let mut notes = Vec::new();
            self.all_notes(&scope.path, 0, &mut notes);
            for path in notes {
                let name_hit = path.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().contains(&needle));
                let mut lines = Vec::new();
                if std::fs::metadata(&path).is_ok_and(|m| m.len() <= MAX_NOTE_BYTES)
                    && let Ok(doc) = Document::open(&path)
                {
                    for (i, line) in doc.text.lines().enumerate() {
                        if line.to_lowercase().contains(&needle) {
                            lines.push((i + 1, snippet(line)));
                        }
                    }
                }
                if !name_hit && lines.is_empty() {
                    continue;
                }
                found += 1;
                if found > MAX_SEARCH_RESULTS {
                    out.push_str(&format!(
                        "… Showing the first {MAX_SEARCH_RESULTS} results. Try a more specific query.\n"
                    ));
                    break 'scopes;
                }
                out.push_str(&self.display(scope.root, &path));
                out.push('\n');
                for (n, text) in lines.iter().take(MAX_SNIPPETS) {
                    out.push_str(&format!("  {n}: {text}\n"));
                }
                if lines.len() > MAX_SNIPPETS {
                    out.push_str(&format!("  … {} more lines\n", lines.len() - MAX_SNIPPETS));
                }
            }
        }
        if found == 0 {
            return Ok(format!("No notes contain '{query}'."));
        }
        Ok(out)
    }

    fn create_note(&self, input: &str, content: &str) -> Result<String, String> {
        let target = self.resolve_note(input)?;
        let shown = self.display(target.root, &target.path);
        if target.path.exists() {
            return Err(format!("File already exists: {shown}. Use update_note or append_to_note to change it."));
        }
        if let Some(parent) = target.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("Can't create the folder: {e}"))?;
        }
        let mut doc = Document::new_untitled(0);
        doc.text = normalize(content);
        save(&mut doc, &target.path)?;
        Ok(format!("Created: {shown}"))
    }

    fn update_note(&self, input: &str, content: &str) -> Result<String, String> {
        let target = self.resolve_note(input)?;
        let mut doc = open_existing(&target.path, input)?;
        doc.text = normalize(content);
        let note = save(&mut doc, &target.path)?;
        Ok(format!("Updated: {}{note}", self.display(target.root, &target.path)))
    }

    fn append_to_note(&self, input: &str, content: &str) -> Result<String, String> {
        let target = self.resolve_note(input)?;
        let mut doc = open_existing(&target.path, input)?;
        if !doc.text.is_empty() && !doc.text.ends_with('\n') {
            doc.text.push('\n');
        }
        doc.text.push_str(&normalize(content));
        let note = save(&mut doc, &target.path)?;
        Ok(format!("Appended: {}{note}", self.display(target.root, &target.path)))
    }
}

/// 파일·폴더 이름 하나가 Windows에서 안전한지 (드라이브 문자, 대체 데이터 스트림, 장치 이름 등 차단)
fn check_name(part: &str) -> Result<(), String> {
    if part.chars().any(|c| c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*')) {
        return Err(format!("The name contains characters that aren't allowed: {part}"));
    }
    let stem = part.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        return Err(format!("This name isn't allowed on Windows: {part}"));
    }
    // 한 덩어리여야 합니다 (드라이브·루트 등이 섞이지 않게).
    let mut comps = Path::new(part).components();
    if !matches!((comps.next(), comps.next()), (Some(Component::Normal(_)), None)) {
        return Err(format!("This name isn't allowed: {part}"));
    }
    Ok(())
}

/// `path`(또는 아직 없으면 가장 가까운 있는 상위 폴더)의 실제 위치가 `root` 안인지
fn within(path: &Path, root: &Path) -> bool {
    let mut probe = path;
    loop {
        if let Ok(canon) = probe.canonicalize() {
            return canon.starts_with(root);
        }
        match probe.parent() {
            Some(parent) => probe = parent,
            None => return false,
        }
    }
}

fn is_note(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| NOTE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

fn open_existing(path: &Path, input: &str) -> Result<Document, String> {
    if !path.is_file() {
        return Err(format!("No such note: {input}. Check the path with list_notes or search_notes."));
    }
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_NOTE_BYTES) {
        return Err(format!("The file is too large (over {}MB): {input}", MAX_NOTE_BYTES / 1024 / 1024));
    }
    Document::open(path).map_err(|e| format!("Can't read: {e}"))
}

/// 앱과 같은 방식(임시 파일에 쓴 뒤 교체, 원래 인코딩·줄 끝 유지)으로 저장합니다.
/// 원래 인코딩(CP949)으로 쓸 수 없는 글자가 있어 UTF-8로 바꿨으면 그 안내를 돌려줍니다.
fn save(doc: &mut Document, path: &Path) -> Result<&'static str, String> {
    let fell_back = doc.save_to(path, WhenBlocked::Fail).map_err(|e| format!("Can't save: {e}"))?;
    Ok(if fell_back {
        " (saved as UTF-8 because some characters can't be written in the original encoding)"
    } else {
        ""
    })
}

/// 줄바꿈을 `\n`으로 (저장할 때 파일의 원래 줄 끝으로 바뀝니다)
fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn snippet(line: &str) -> String {
    const MAX: usize = 160;
    let line = line.trim();
    if line.chars().count() <= MAX {
        line.to_owned()
    } else {
        format!("{}…", line.chars().take(MAX).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::LineEnding;
    use crate::encoding::TextEncoding;

    /// 테스트마다 따로 쓰는 빈 메모 폴더
    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tb-notepad-mcp-{}", std::process::id())).join(name).join("Notes");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn vault(root: &Path) -> Vault {
        Vault::new(&[root.to_path_buf()], "images")
    }

    #[test]
    fn paths_cannot_escape_the_notes_folder() {
        let root = temp_root("escape");
        std::fs::write(root.parent().unwrap().join("secret.md"), "비밀").unwrap();
        let v = vault(&root);
        for bad in [
            "../secret.md",
            "Notes/../secret.md",
            "sub/../../secret.md",
            "..\\secret.md",
            "/etc/passwd.md",
            "\\secret.md",
            "C:/Windows/win.md",
            "C:secret.md",
            "\\\\server\\share\\x.md",
            "note.md:stream",
            "a:b.md",
            "CON.md",
            "sub/nul.txt",
        ] {
            assert!(v.resolve_note(bad).is_err(), "{bad} 는 거부되어야 함");
        }
        assert!(v.read_note("../secret.md").is_err());
        assert!(v.create_note("../evil.md", "x").is_err());
        assert!(!root.parent().unwrap().join("evil.md").exists());
    }

    #[test]
    fn only_md_and_txt() {
        let root = temp_root("ext");
        let v = vault(&root);
        assert!(v.create_note("run.bat", "x").is_err());
        assert!(v.create_note("noext", "x").is_err());
        assert!(v.create_note("a.MD", "x").is_ok());
        assert!(v.create_note("b.txt", "x").is_ok());
        std::fs::write(root.join("c.json"), "{}").unwrap();
        assert!(v.read_note("c.json").is_err());
        assert!(!v.list_notes(None).unwrap().contains("c.json"));
    }

    #[test]
    fn root_name_prefix_is_optional_with_one_root() {
        let root = temp_root("prefix");
        let v = vault(&root);
        v.create_note("Notes/일기/첫날.md", "안녕").unwrap();
        assert_eq!(v.read_note("일기/첫날.md").unwrap(), "안녕");
        assert_eq!(v.read_note("notes\\일기\\첫날.md").unwrap(), "안녕");
        assert_eq!(v.read_note("./Notes/일기/첫날.md").unwrap(), "안녕");
    }

    #[test]
    fn several_roots_need_a_prefix() {
        let a = temp_root("multi-a");
        let b = temp_root("multi-b"); // 둘 다 "Notes" → 두 번째는 "Notes (2)"
        let v = Vault::new(&[a.clone(), b.clone()], "images");
        v.create_note("Notes (2)/b.md", "B").unwrap();
        assert!(b.join("b.md").exists());
        assert!(v.create_note("b.md", "x").is_err(), "폴더가 여럿이면 이름을 꼭 써야 함");
        let list = v.list_notes(None).unwrap();
        assert!(list.contains("Notes/\n") && list.contains("Notes (2)/b.md"));
    }

    #[test]
    fn junction_out_of_root_is_rejected() {
        let root = temp_root("link");
        let outside = root.parent().unwrap().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("x.md"), "밖").unwrap();
        let link = root.join("link");
        #[cfg(windows)]
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .is_ok_and(|o| o.status.success());
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&outside, &link).is_ok();
        if !made {
            eprintln!("링크를 만들 수 없어 건너뜀");
            return;
        }
        let v = vault(&root);
        assert!(v.read_note("link/x.md").is_err());
        assert!(v.create_note("link/y.md", "x").is_err());
        assert!(!outside.join("y.md").exists());
        assert!(!v.list_notes(None).unwrap().contains("x.md"));
        assert!(v.search_notes("밖", None).unwrap().starts_with("No notes"));
    }

    #[test]
    fn create_read_append_update() {
        let root = temp_root("crud");
        let v = vault(&root);
        v.create_note("할 일.md", "# 할 일\n- 우유").unwrap();
        assert!(v.create_note("할 일.md", "덮어쓰기").is_err(), "이미 있으면 실패");
        v.append_to_note("할 일.md", "- 빵\n").unwrap();
        assert_eq!(v.read_note("할 일.md").unwrap(), "# 할 일\n- 우유\n- 빵\n");
        v.update_note("할 일.md", "끝").unwrap();
        assert_eq!(v.read_note("할 일.md").unwrap(), "끝");
        assert!(v.update_note("없음.md", "x").is_err());
        assert!(v.append_to_note("없음.md", "x").is_err());
        // 임시 파일이 남지 않음
        let names: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(names.len(), 1, "{names:?}");
    }

    #[test]
    fn writes_keep_encoding_and_line_endings() {
        let root = temp_root("encoding");
        let v = vault(&root);
        let (bytes, _, _) = encoding_rs::EUC_KR.encode("첫 줄\r\n둘째 줄\r\n");
        std::fs::write(root.join("옛날.txt"), &bytes).unwrap();
        v.append_to_note("옛날.txt", "셋째 줄\n").unwrap();
        let doc = Document::open(&root.join("옛날.txt")).unwrap();
        assert_eq!(doc.encoding, TextEncoding::EucKr);
        assert_eq!(doc.line_ending, LineEnding::CrLf);
        assert_eq!(doc.text, "첫 줄\n둘째 줄\n셋째 줄\n");

        // CP949로 못 쓰는 글자는 UTF-8로 바꿔 저장하고 알려 줌
        let msg = v.update_note("옛날.txt", "이모지 😀").unwrap();
        assert!(msg.contains("UTF-8"));
        assert_eq!(v.read_note("옛날.txt").unwrap(), "이모지 😀");

        let mut utf16 = vec![0xFF, 0xFE];
        utf16.extend("a\nb".encode_utf16().flat_map(u16::to_le_bytes));
        std::fs::write(root.join("u16.md"), &utf16).unwrap();
        v.update_note("u16.md", "가\n나").unwrap();
        let doc = Document::open(&root.join("u16.md")).unwrap();
        assert_eq!((doc.encoding, doc.line_ending), (TextEncoding::Utf16Le, LineEnding::Lf));
        assert_eq!(doc.text, "가\n나");
    }

    #[test]
    fn list_and_search() {
        let root = temp_root("search");
        let v = vault(&root);
        v.create_note("회의/10월 회의.md", "# 회의\n예산 검토\n다음 주 발표").unwrap();
        v.create_note("메모 2.md", "그냥 메모").unwrap();
        v.create_note("메모 10.md", "Budget 예산").unwrap();
        std::fs::create_dir_all(root.join("images")).unwrap();
        std::fs::write(root.join("images").join("숨김.md"), "x").unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();

        let list = v.list_notes(None).unwrap();
        assert_eq!(list, "Notes/\n  Notes/회의/\n    Notes/회의/10월 회의.md\n  Notes/메모 2.md\n  Notes/메모 10.md\n");
        assert_eq!(v.list_notes(Some("Notes/회의")).unwrap(), "Notes/회의/\n  Notes/회의/10월 회의.md\n");
        assert!(v.list_notes(Some("없는 폴더")).is_err());

        let found = v.search_notes("예산", None).unwrap();
        assert!(found.contains("Notes/회의/10월 회의.md\n  2: 예산 검토"), "{found}");
        assert!(found.contains("Notes/메모 10.md\n  1: Budget 예산"), "{found}");
        assert!(v.search_notes("budget", None).unwrap().contains("메모 10.md"), "대소문자 무시");
        assert!(v.search_notes("10월", None).unwrap().contains("10월 회의.md"), "이름으로도 찾음");
        assert!(v.search_notes("예산", Some("회의")).unwrap().lines().count() == 2);
        assert!(v.search_notes("없는말", None).unwrap().starts_with("No notes"));
    }

    fn server(root: &Path) -> Server {
        Server { roots_override: Some(vec![root.to_path_buf()]) }
    }

    fn call(s: &Server, id: i64, name: &str, args: Value) -> Value {
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": name, "arguments": args } });
        s.handle_line(&msg.to_string()).unwrap()
    }

    #[test]
    fn json_rpc_flow() {
        let root = temp_root("rpc");
        let s = server(&root);
        let init = s
            .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#)
            .unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert!(init["result"]["capabilities"]["tools"].is_object());
        // 모르는 버전이면 우리가 아는 최신
        let init = s
            .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#)
            .unwrap();
        assert_eq!(init["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);

        assert!(s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());

        let list = s.handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#).unwrap();
        let names: Vec<&str> =
            list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["list_notes", "read_note", "search_notes", "create_note", "update_note", "append_to_note"]);

        let r = call(&s, 3, "create_note", json!({ "path": "Notes/a.md", "content": "hello" }));
        assert_eq!(r["id"], 3);
        assert_eq!(r["result"]["isError"], false);
        let r = call(&s, 4, "read_note", json!({ "path": "a.md" }));
        assert_eq!(r["result"]["content"][0]["text"], "hello");
        let r = call(&s, 5, "read_note", json!({ "path": "../a.md" }));
        assert_eq!(r["result"]["isError"], true);
        let r = call(&s, 6, "read_note", json!({}));
        assert_eq!(r["result"]["isError"], true);
        let r = call(&s, 7, "delete_everything", json!({}));
        assert_eq!(r["error"]["code"], -32602);

        let r = s.handle_line(r#"{"jsonrpc":"2.0","id":"x","method":"resources/list"}"#).unwrap();
        assert_eq!((r["id"].as_str(), r["error"]["code"].as_i64()), (Some("x"), Some(-32601)));
        let r = s.handle_line("{not json").unwrap();
        assert_eq!(r["error"]["code"], -32700);
        let r = s.handle_line(r#"[{"jsonrpc":"2.0","id":1,"method":"ping"},{"jsonrpc":"2.0","method":"x"}]"#).unwrap();
        assert_eq!(r.as_array().unwrap().len(), 1);
    }
}
