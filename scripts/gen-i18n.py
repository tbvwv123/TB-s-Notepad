"""scripts/i18n_strings.py 의 번역표로 src/i18n_table.rs 를 만듭니다.

코드의 t!("…") / tf!("…") 를 모두 찾아, 번역이 빠진 문구와 더는 안 쓰는 번역을 알려 줍니다.
    python scripts/gen-i18n.py
"""
import glob
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, "scripts"))
from i18n_strings import T  # noqa: E402

# 코드에서 t!() 대신 tr() 로 번역하는 문구 (const 표에 있어서)
EXTRA_KEYS = {
    # src/app/slash.rs 의 COMMANDS 이름
    "제목 1", "제목 2", "제목 3", "글머리 목록", "번호 목록", "할 일", "인용", "작은 글씨", "코드 블록", "표",
    "구분선", "웹 링크", "메모 링크", "이 줄 링크 복사", "파일 링크", "폴더 링크", "그림 · 소리 · 영상",
    "클립보드 그림 붙여넣기", "날짜와 시간", "오늘 날짜",
}

PAT = re.compile(r'\b(?:t|tf)!\(\s*"((?:[^"\\]|\\.)*)"', re.S)


def unescape(lit):
    """러스트 문자열 리터럴 내용 → 실제 문자열"""
    out, i = [], 0
    while i < len(lit):
        c = lit[i]
        if c != "\\":
            out.append(c)
            i += 1
            continue
        n = lit[i + 1]
        if n == "\n":  # 줄 이어 쓰기: 다음 줄 앞 공백까지 건너뜀
            i += 2
            while i < len(lit) and lit[i] in " \t\r\n":
                i += 1
            continue
        if n == "u":
            end = lit.index("}", i)
            out.append(chr(int(lit[i + 3:end], 16)))
            i = end + 1
            continue
        out.append({"n": "\n", "t": "\t", "r": "\r", "0": "\0", "\\": "\\", '"': '"', "'": "'"}[n])
        i += 2
    return "".join(out)


def rust_str(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n") + '"'


def main():
    used = set(EXTRA_KEYS)
    for f in glob.glob(os.path.join(ROOT, "src", "**", "*.rs"), recursive=True):
        if os.path.basename(f) in ("i18n.rs", "i18n_table.rs"):
            continue
        s = open(f, encoding="utf-8").read()
        cut = s.find("#[cfg(test)]")
        s = s if cut < 0 else s[:cut]
        used.update(unescape(m) for m in PAT.findall(s))

    missing = sorted(used - T.keys())
    unused = sorted(T.keys() - used)
    bad = [k for k, v in T.items() if any(x.count("{}") != k.count("{}") for x in v)]
    for k in missing:
        print("번역 없음:", repr(k))
    for k in unused:
        print("안 쓰는 번역:", repr(k))
    for k in bad:
        print("{} 개수 다름:", repr(k))

    lines = [
        "// 자동 생성: python scripts/gen-i18n.py (scripts/i18n_strings.py 를 고친 뒤 다시 실행)",
        "// (한국어, English, 中文, 日本語)",
        "pub const ROWS: &[(&str, &str, &str, &str)] = &[",
    ]
    for k in sorted(T):
        en, zh, ja = T[k]
        lines.append(f"    ({rust_str(k)}, {rust_str(en)}, {rust_str(zh)}, {rust_str(ja)}),")
    lines.append("];")
    open(os.path.join(ROOT, "src", "i18n_table.rs"), "w", encoding="utf-8", newline="\n").write("\n".join(lines) + "\n")
    print(f"{len(T)}개 문구 → src/i18n_table.rs")
    sys.exit(1 if missing or bad else 0)


main()
