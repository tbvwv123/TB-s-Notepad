"""TB-Notepad.exe --mcp 를 stdio로 띄워 initialize → tools/list → tools/call 을 보내 봅니다.

사용법:  python -X utf8 scripts\\mcp-smoke.py dist\\TB-Notepad.exe
임시 폴더(TB_NOTEPAD_DATA)를 쓰므로 실제 설정과 메모는 건드리지 않습니다.
"""
import json, os, subprocess, sys, tempfile

exe = os.path.abspath(sys.argv[1])
data = tempfile.mkdtemp(prefix="tb-mcp-smoke-")
env = dict(os.environ, TB_NOTEPAD_DATA=data)  # 실제 설정·메모는 건드리지 않음
p = subprocess.Popen([exe, "--mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, env=env)

def send(msg, expect_reply=True):
    p.stdin.write((json.dumps(msg, ensure_ascii=False) + "\n").encode("utf-8"))
    p.stdin.flush()
    if expect_reply:
        reply = json.loads(p.stdout.readline())
        print(">>", msg.get("method"), (msg.get("params") or {}).get("name", ""))
        print(json.dumps(reply, ensure_ascii=False, indent=1)[:900])
        return reply

send({"jsonrpc": "2.0", "id": 1, "method": "initialize",
      "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "smoke", "version": "1"}}})
send({"jsonrpc": "2.0", "method": "notifications/initialized"}, expect_reply=False)
send({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
n = 3
def call(name, **args):
    global n; n += 1
    return send({"jsonrpc": "2.0", "id": n, "method": "tools/call", "params": {"name": name, "arguments": args}})
call("create_note", path="TB Notes/AI/회의록.md", content="# 회의록\n- 예산 확정")
call("append_to_note", path="TB Notes/AI/회의록.md", content="- 다음 회의: 금요일")
call("list_notes")
call("search_notes", query="예산")
call("read_note", path="AI/회의록.md")
call("read_note", path="../../Windows/win.ini")
p.stdin.close()
print("exit code:", p.wait(timeout=5))
print("on disk:", repr(open(os.path.join(data, "TB Notes", "AI", "회의록.md"), "rb").read().decode("utf-8")))
