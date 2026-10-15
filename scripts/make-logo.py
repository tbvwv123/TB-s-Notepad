"""TB's Notepad 로고 만들기: 검정 둥근 타일 + 흰 "TB" + 빨간 입력 커서.

    python scripts/make-logo.py              # assets/logo.png, assets/logo.ico 만들기
    python scripts/make-logo.py --preview 파일.png   # 크기별 미리보기 (밝은/어두운 바탕)

logo.png (256px)는 창 아이콘에, logo.ico (16~256px)는 실행 파일 아이콘(build.rs)에 들어갑니다.
"""

import os
import sys

from PIL import Image, ImageDraw, ImageFont

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
ASSETS = os.path.join(ROOT, "assets")
FONT = os.path.join(os.environ.get("WINDIR", "C:/Windows"), "Fonts", "segoeuib.ttf")  # Segoe UI Bold

BG_TOP = (30, 30, 33)
BG_BOTTOM = (12, 12, 13)
EDGE = (52, 52, 58)  # 검정 작업 표시줄 위에서도 타일 모양이 보이게 하는 얇은 테두리
INK = (240, 240, 242)
RED = (235, 51, 45)

SS = 4  # 4배로 그린 뒤 줄여서 가장자리를 매끄럽게
ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]


def tile(s):
    """검정 둥근 타일 (위가 살짝 밝은 세로 그라데이션 + 얇은 테두리)"""
    grad = Image.new("RGB", (1, s))
    for y in range(s):
        t = y / max(1, s - 1)
        grad.putpixel((0, y), tuple(round(a + (b - a) * t) for a, b in zip(BG_TOP, BG_BOTTOM)))
    grad = grad.resize((s, s))
    radius = round(s * 0.225)
    mask = Image.new("L", (s, s), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, s - 1, s - 1), radius=radius, fill=255)
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    img.paste(grad, (0, 0), mask)
    ImageDraw.Draw(img).rounded_rectangle(
        (0, 0, s - 1, s - 1), radius=radius, outline=EDGE + (255,), width=max(SS, round(s * 0.008))
    )
    return img


def logo(size):
    s = size * SS
    img = tile(s)
    d = ImageDraw.Draw(img)
    u = s / 1024
    font = ImageFont.truetype(FONT, round(470 * u))
    d.text((170 * u, 512 * u), "TB", font=font, fill=INK, anchor="lm")
    # 입력 커서: 글자 바로 뒤에 서 있는 빨간 막대
    d.rounded_rectangle((770 * u, 300 * u, 836 * u, 724 * u), radius=33 * u, fill=RED)
    return img.resize((size, size), Image.LANCZOS)


def preview(path):
    sizes = [256, 64, 48, 32, 24, 16]
    w = sum(sizes) + 40 * (len(sizes) + 1)
    h = 256 + 60
    out = Image.new("RGB", (w, h * 2), (245, 245, 247))
    out.paste(Image.new("RGB", (w, h), (32, 32, 36)), (0, h))
    for band in (0, 1):
        x = 40
        for sz in sizes:
            ic = logo(sz)
            out.paste(ic, (x, band * h + 30 + (256 - sz) // 2), ic)
            x += sz + 40
    out.save(path)


def main():
    if len(sys.argv) > 2 and sys.argv[1] == "--preview":
        preview(sys.argv[2])
        return
    os.makedirs(ASSETS, exist_ok=True)
    logo(256).save(os.path.join(ASSETS, "logo.png"))
    images = [logo(sz) for sz in ICO_SIZES]
    images[-1].save(
        os.path.join(ASSETS, "logo.ico"), sizes=[(sz, sz) for sz in ICO_SIZES], append_images=images[:-1]
    )
    print("assets/logo.png, assets/logo.ico 를 만들었습니다")


if __name__ == "__main__":
    main()
