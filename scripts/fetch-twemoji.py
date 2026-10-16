"""기본 트위모지 그림을 내려받고 Rust 표를 만듭니다.

- 그림: jdecked/twemoji v15.1.0 의 72x72 PNG → assets/twemoji/<코드>.png (이미 있으면 건너뜀)
- 표: src/twemoji_table.rs (이름, 문자, 그림 바이트)
트위모지 그림은 CC-BY 4.0 (assets/twemoji/NOTICE.md).
"""
import os
import sys
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, "scripts"))
from twemoji_list import EMOJI  # noqa: E402

TAG = "v15.1.0"
BASE = f"https://raw.githubusercontent.com/jdecked/twemoji/{TAG}/assets/72x72"
OUT = os.path.join(ROOT, "assets", "twemoji")
os.makedirs(OUT, exist_ok=True)

fetched = 0
for name, cp in EMOJI:
    path = os.path.join(OUT, f"{cp}.png")
    if os.path.exists(path):
        continue
    with urllib.request.urlopen(f"{BASE}/{cp}.png", timeout=30) as r:
        data = r.read()
    if not data.startswith(b"\x89PNG"):
        raise SystemExit(f"{name} ({cp}): PNG 가 아님")
    open(path, "wb").write(data)
    fetched += 1

lines = [
    "// 자동 생성: python scripts/fetch-twemoji.py (목록은 scripts/twemoji_list.py)",
    "// 트위모지 그림: jdecked/twemoji " + TAG + ", CC-BY 4.0 (assets/twemoji/NOTICE.md)",
    "",
    "/// (짧은 이름, 문자, 72x72 PNG)",
    "#[rustfmt::skip]",
    "pub const TWEMOJI: &[(&str, &str, &[u8])] = &[",
]
for name, cp in EMOJI:
    code = int(cp, 16)
    # 기호 영역(U+2000~)의 글자는 그림 모양으로 보이게 VS16(FE0F)을 붙임
    text = "".join(f"\\u{{{code:x}}}" + ("\\u{fe0f}" if code < 0x1F000 else ""))
    lines.append(f'    ("{name}", "{text}", include_bytes!("../assets/twemoji/{cp}.png")),')
lines.append("];")
open(os.path.join(ROOT, "src", "twemoji_table.rs"), "w", encoding="utf-8", newline="\n").write("\n".join(lines) + "\n")
total = sum(os.path.getsize(os.path.join(OUT, f"{cp}.png")) for _, cp in EMOJI)
print(f"{len(EMOJI)}개 (새로 받은 것 {fetched}개, 모두 {total / 1024:.0f} KB) → src/twemoji_table.rs")
