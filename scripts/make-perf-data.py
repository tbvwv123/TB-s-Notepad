"""성능 시험용 데이터: 크기별 큰 문서 (100KB, 500KB, 950KB, 3MB) + 메모 3000개 (30폴더 x 100).

    python scripts/make-perf-data.py <폴더>      (기본: target 옆이 아니라 ./perf_data)

만든 폴더를 TB_NOTEPAD_DATA 로 쓰면 됩니다. 자세한 측정 방법은 docs/DEVELOPMENT.md 의 "Performance".
"""
import os
import random
import shutil
import sys

data = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "perf_data")
if os.path.exists(data):
    shutil.rmtree(data)
notes = os.path.join(data, "TB Notes")
big = os.path.join(notes, "큰 문서")
many = os.path.join(notes, "메모 3000")
os.makedirs(big)
random.seed(42)

WORDS_KO = "오늘 회의 결과 정리 다음 주 일정 예산 검토 디자인 수정 사용자 피드백 반영 출시 준비 문서 작성 테스트 확인 메모장 기능 성능 개선".split()
WORDS_EN = "the quick brown fox jumps over lazy dog release notes budget review design feedback performance editor markdown".split()


def sentence(n):
    out = []
    for i in range(n):
        w = random.choice(WORDS_KO if random.random() < 0.6 else WORDS_EN)
        r = random.random()
        if r < 0.05:
            w = f"**{w}**"
        elif r < 0.08:
            w = f"*{w}*"
        elif r < 0.10:
            w = f"`{w}`"
        out.append(w)
    return " ".join(out) + "."


def section(i):
    lines = [f"## {i}. {sentence(4)[:-1]}", "", sentence(30), "", sentence(25), ""]
    lines += [f"- {sentence(8)}" for _ in range(4)] + ["  - " + sentence(6), ""]
    lines += [f"- [{'x' if k % 2 else ' '}] {sentence(5)}" for k in range(3)] + [""]
    lines += [f"{k}. {sentence(7)}" for k in range(1, 4)] + [""]
    lines += [f"> {sentence(12)}", "", f"-# {sentence(6)}", ""]
    if i % 3 == 0:
        lines += ["```rust", "fn main() {", f'    println!("{i}");', "}", "```", ""]
    if i % 4 == 0:
        lines += ["| 항목 | 값 | 메모 |", "| --- | :---: | ---: |"]
        lines += [f"| {random.choice(WORDS_KO)} | {k * 7} | {sentence(3)} |" for k in range(5)] + [""]
    if i % 5 == 0:
        lines += ["<details>", f"<summary>{sentence(3)[:-1]}</summary>", "", sentence(20), "", "</details>", ""]
    lines += [f"[링크 {i}](https://example.com/{i}) · [위로](#L1) · [다른 메모](../메모%203000/폴더%2001/메모%20001.md)", "", "---", ""]
    return "\n".join(lines) + "\n"


def make(name, target_bytes):
    parts, size, i = ["# 성능 시험 문서\n\n"], 0, 1
    while size < target_bytes:
        s = section(i)
        if size + len(s.encode()) > target_bytes and size > 0:
            break
        parts.append(s)
        size += len(s.encode())
        i += 1
    text = "".join(parts)
    with open(os.path.join(big, name), "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
    return len(text.encode()), text.count("\n"), i - 1


for name, kb in [("100KB.md", 100), ("500KB.md", 500), ("950KB.md", 950), ("3MB.md", 3000)]:
    b, lines, sections = make(name, kb * 1024 if kb < 3000 else 3 * 1024 * 1024)
    print(f"{name}: {b / 1024:.0f} KB, {lines} lines, {sections} sections")

# 메모 3000개
for f in range(1, 31):
    d = os.path.join(many, f"폴더 {f:02d}")
    os.makedirs(d)
    for n in range(1, 101):
        body = f"# 메모 {f:02d}-{n:03d}\n\n" + "\n\n".join(sentence(30) for _ in range(6)) + "\n"
        if (f * 100 + n) % 250 == 0:
            body += "\n이 메모에는 찾기 시험용 낱말 zebra42 가 들어 있습니다.\n"
        with open(os.path.join(d, f"메모 {n:03d}.md"), "w", encoding="utf-8", newline="\n") as fh:
            fh.write(body)
print("3000 notes in 메모 3000")

BS = chr(92)


def ron_str(x):
    return '"' + x.replace(BS, BS + BS).replace('"', BS + '"') + '"'


settings = "(roots_initialized:true,roots:[%s])" % ron_str(notes)
with open(os.path.join(data, "app.ron"), "w", encoding="utf-8") as f:
    f.write("{\n    \"settings\": %s,\n}\n" % ron_str(settings))
