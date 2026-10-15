# Contributing

**English** · [한국어](#한국어)

Thanks for your interest in TB's Notepad!

- **Bugs and ideas** — open an [issue](https://github.com/tbvwv123/TB-s-Notepad/issues/new/choose). Any language is fine: English, 한국어, 中文, 日本語.
- **Translations** — UI text lives in [`scripts/i18n_strings.py`](scripts/i18n_strings.py) (Korean → English, 简体中文, 日本語).
  After editing, run `python scripts/gen-i18n.py`; it regenerates `src/i18n_table.rs` and lists missing translations.
  The first-run guide note is in [`samples/welcome/`](samples/welcome), and the READMEs are `README*.md`.
- **Code** — see [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for building and the code layout.
  Before sending a pull request, please run:

  ```powershell
  cargo fmt
  cargo clippy --all-targets
  cargo test
  python scripts/gen-i18n.py
  ```

  The code comments are in Korean; English comments are welcome in new code.

By contributing you agree that your work is released under the [MIT License](LICENSE).

---

## 한국어

TB's Notepad 에 관심 가져 주셔서 고맙습니다!

- **버그와 아이디어** — [이슈](https://github.com/tbvwv123/TB-s-Notepad/issues/new/choose)로 알려 주세요. 어느 언어로 쓰셔도 됩니다.
- **번역** — 화면 문구는 [`scripts/i18n_strings.py`](scripts/i18n_strings.py) 에 있습니다 (한국어 → English, 简体中文, 日本語).
  고친 뒤 `python scripts/gen-i18n.py` 를 실행하면 `src/i18n_table.rs` 를 다시 만들고 빠진 번역을 알려 줍니다.
  처음 실행할 때 나오는 안내 메모는 [`samples/welcome/`](samples/welcome), README 는 `README*.md` 입니다.
- **코드** — 빌드 방법과 코드 구조는 [docs/DEVELOPMENT.ko.md](docs/DEVELOPMENT.ko.md) 를 보세요.
  풀 리퀘스트 전에 위의 네 명령을 실행해 주세요.

기여한 내용은 [MIT 라이선스](LICENSE)로 공개되는 데 동의하는 것으로 봅니다.
