# Welcome to TB's Notepad

A light, easy notepad where formatting shows up as you type. This one note covers the basics and every bit of markdown you can use.

![TB's Notepad](images/welcome.png "align=center")

-# This is an ordinary note, too. Feel free to edit or delete it.

## First steps

- [x] Get TB's Notepad
- [ ] Make your first note with **New note** in the left sidebar
- [ ] Switch between light and dark with ☀ / 🌙 at the top right
- [ ] Click the picture above and change its size and position
- [ ] Type `/` on an empty line to open the command list
- [ ] Press Ctrl+E to see this note's source (the markdown symbols)

---

## A quick tour

- **Left sidebar**: search, new note, add folder, and your folders › subfolders › notes
- **Top bar**: where this note lives (folder / name), find, source view, light / dark, settings, ⋯ menu
- **Formatting toolbar**: bold · italic · strikethrough · code · headings 1–3 · lists · to-do · quote · code block · table · divider · link · image · audio · video · file link
- **Status bar**: line · column, character count, save state, encoding, line endings, text size

## Making and organizing notes

- **New note** (Ctrl+N): created right in the folder you're looking at. The first line you type becomes the file name.
- **Autosave**: saved as soon as you stop typing. No need to save by hand.
- **Add folder**: adds a folder for your notes to the sidebar. You can also drag a folder onto the window.
- The **+** next to a folder makes a new note in it. Folders can hold as many subfolders as you like.
- **Right-click** a note or folder for new folder, rename (F2), show in Explorer, and move to Recycle Bin.
- **Search** (Ctrl+P): looks through file names and contents in every folder.
- **Drag a note or folder** in the sidebar onto another folder to move it. Images and links in the note, and links in other notes pointing to it, are updated too.
- Open files from elsewhere by dragging them onto the window or with ⋯ › Open file (Ctrl+O). They show up under "Open documents" in the sidebar.

## `/` commands

No need to remember shortcuts. Type `/` at the start of a line or after a space and a list of things you can do pops up.

- Keep typing to filter the list: `/head`, `/link`, `/h2`, `/todo`.
- Pick with ↑ ↓ and run with **Enter** (or Tab, or a click). The `/text` you typed is removed.
- Type a **space** or press Esc to close the list; the `/` stays as plain text.
- Commands: headings 1–3 · bulleted list · numbered list · to-do · quote · small text · code block · table · divider · web link · note link · copy link to this line · file link · folder link · image · audio · video · clipboard image · date and time · today's date
- A `/` right after a letter, as in `10/15` or `a/b`, doesn't open the list.

## Lists and toggles

- Press **Enter** on a bulleted, numbered, to-do or quote line and the next line gets the same marker. Press it again on an empty item to end the list.
- **Tab** / **Shift+Tab** indent and outdent list items.
- Type `>>` and a space at the start of a line to make a **toggle** (a block that folds open and closed). You can also use `/toggle` or the Format menu.
- Click the ▶ in front of the title, or press **Ctrl+Enter** on the title line, to fold and unfold it. Anything can go inside — lists, images and more.
- Toggles are saved as `<details>`, which also folds on GitHub and in VS Code.

---

## Markdown syntax

Formatting appears as soon as you type the symbols, and the symbols only show (faintly) on the line with the cursor. You can also use the toolbar or shortcuts.

### Text styles

- `**bold**` → **bold** (Ctrl+B)
- `*italic*` → *italic* (Ctrl+I)
- `~~strikethrough~~` → ~~strikethrough~~
- `` `code` `` → `code`
- `[link text](https://example.com)` → [link text](https://example.com) (Ctrl+K, open with **Ctrl+click**)
- A bare address becomes a link too: https://example.com
- `[Minutes](meetings/october.md)` → a link to another note. **Ctrl+click** opens it
- `[Decisions](meetings/october.md#decisions)` · `[Line 12](meetings/october.md#L12)` → jump straight to a heading or line in that note
- `[Back to top](#L1)` · `[Markdown syntax](#markdown-syntax)` → move within this note: [go to Markdown syntax](#markdown-syntax)
- `[Meetings folder](meetings)` → a folder inside your notes folder opens in the sidebar
- `[Report](C:/Files/report.pdf)` → other files and folders open in Explorer (wrap paths with spaces in `<...>`)

### Line styles (at the start of a line)

- `# Heading` · `## Heading` · `### Heading` → large to small headings (Ctrl+Alt+1–3)
- `- item` → bulleted list (indent two spaces for a sub-list)
- `1. item` → numbered list
- `- [ ] to do` · `- [x] done` → checkbox (click to check)
- `> quote` → a quote with a bar on the left
- `-# small text` → small gray side note
- `>> Title` → a toggle that folds open and closed (it changes as soon as you type the space)
- `---` → horizontal divider

### Multi-line styles

- Code block: wrap lines with a line of three backticks (```) above and below. You can put a language name after the first one.
- Table: split cells with bars like `| cell | cell |`. The second line is `| --- | --- |`; `:---:` centers and `---:` right-aligns. It shows as a table while the cursor is elsewhere, and turns back into source when you click it so you can edit. Insert one quickly with `/table`.

### Images · audio · video

- `![description](images/picture.png)` → a line with only an image shows the image right there.
- `![description](images/picture.png "width=50% align=center")` → size (% of text width) and position (left · center · right)
- `![description](music/song.mp3)` → an audio file becomes an audio card with a progress bar (mp3 · wav · flac · ogg · m4a · aac)
- `![description](video/clip.mp4)` → a video file becomes a video card showing its first frame (mp4 · webm · mkv · mov · avi · wmv)

### What it looks like

#### Lists and to-dos

- Apples
  - Fuji
  - Gala
- Pears

1. Boil water
2. Add noodles
3. Wait three minutes

- [x] Groceries
- [ ] Dishes

#### Quotes and small text

> Good tools stay out of the way.

-# Small text is handy for side notes and explanations.

#### Code block

```rust
fn main() {
    println!("Hello!");
}
```

#### Table

| Feature | Shortcut |
| --- | --- |
| Find | Ctrl+F |
| Replace | Ctrl+H |

#### Toggle

<details>
<summary>Click to unfold</summary>

This was folded away. Toggles can hold lists and images too.

</details>

---

## Adding images

- **Copy an image and press Ctrl+V**: it's saved as a PNG in an `images` folder next to the note and appears right away.
- You can also **drag image files** onto the window, or pick a file with the toolbar's image button.
- **Click an image** for its toolbar: 25 · 50 · 75 · 100% · original size, left · center · right, delete
- **Drag the corner** of a selected image to resize it, like in Word. Delete removes it, Esc deselects it.
- The cursor only stops before or after an image. Press Enter after an image to start a new line below it.
- Images from elsewhere are copied into the note's folder when you add them or save (you can turn this off in Settings).

## Audio and video

- Pick audio or video files with the toolbar's image button (or ⋯ › Edit › Insert image · audio · video), or **drag them onto the window**.
- **Audio** plays right in the note: ▶ to play or pause, click or drag the progress bar to jump.
- Use the **speaker and small slider** on the right of an audio card to change the volume. Click the speaker to mute and click again to unmute. The volume is remembered.
- **Video** shows as a card with its first frame. Click ▶ in the middle or double-click the card to open it in your default video player.
- Cards can be resized, moved and deleted just like images.
- Audio and video files are large, so they aren't copied into the notes folder; the note points to the original file. If you move the file, the card says "File not found".

## Links between notes

- **Insert note link** (Ctrl+Shift+K, or ⋯ › Edit): search by note or folder name and pick one to insert a link.
- **Copy link to this line** (Ctrl+Shift+L): copies a link to the line with the cursor. On a heading line it links to that heading. Paste it into another note with Ctrl+V and the path is adjusted for that note.
- You can also **right-click › Copy hyperlink** on a note or folder in the sidebar.
- Select some text and **right-click › Copy hyperlink** to copy a link to that line with the selected text as its label. The right-click menu also has cut · copy · paste · make link.
- **Ctrl+click** a link: notes open in the app (at that line or heading), folders unfold in the sidebar.

## File and folder links

- Use the toolbar's folder button (or ⋯ › Edit › Insert file link · Insert folder link) to pick a file or folder on this PC and insert a link.
- Dragging in files that can't be opened as notes (pdf, Excel, zip…) also inserts links. Hold **Shift** while dropping a folder to insert a link to it.
- You can **copy files in Explorer and press Ctrl+V** too: images, audio and video show up as they are; everything else becomes a link.
- **Ctrl+click** a link: folders open in Explorer, and files open in Explorer with the file selected. Notes and folders inside your notes folder open in the app.

## View and style

- **Source view** (Ctrl+E): edit with the markdown symbols visible. Press again to go back.
- **Themes**: light / dark with ☀ / 🌙 at the top right; 8 themes and custom colors in Settings › Theme & colors
- **Fonts**: body text, source view and interface fonts plus scale in Settings › Fonts
- **Text size**: Ctrl+mouse wheel, Ctrl+= / Ctrl+-, Ctrl+0 resets
- Toggle the **sidebar**: Ctrl+\
- More in Settings › Editor: language, line numbers, text width limit, toolbar, status bar, always on top
- Full screen: F11

## Find and files

- Find Ctrl+F, replace Ctrl+H, next / previous F3 · Shift+F3
- Insert date and time: F5
- .txt files open as plain text and are saved back to the same file with the same encoding (UTF-8, ANSI…) and line endings.
- Change the encoding and line endings (CRLF · LF) by clicking them in the status bar.
- ⋯ › Export as HTML saves a web page. Audio and video use the browser's player.
- **Previous versions** (⋯ › Edit › Previous versions, or right-click in the sidebar): see and restore what was kept every 5 minutes while you edit, and whenever a lot was deleted.
- If a note changes outside the app, it's reloaded automatically. If it changed in both places, you're asked which to keep.

## Shortcuts

- Ctrl+N new note · Ctrl+O open file · Ctrl+S save · Ctrl+W close document
- Ctrl+P search notes · Ctrl+\ sidebar · F2 rename
- Ctrl+B bold · Ctrl+I italic · Ctrl+K link · Ctrl+Alt+1–3 headings
- Ctrl+Shift+K insert note link · Ctrl+Shift+L copy link to this line · Ctrl+click follow a link
- `/` command list (find and run commands by name instead of shortcuts)
- Enter continues lists · Tab / Shift+Tab indent · Ctrl+Enter folds a toggle
- Ctrl+V paste (images too) · Ctrl+Z / Ctrl+Y undo / redo
- Ctrl+E source view · Ctrl+, settings · F11 full screen

## Working with AI

AI apps like Claude can read and write the notes on this PC (no cloud, no login). Register `TB-Notepad.exe --mcp` in the AI app's settings and it can list, read, search, create and edit notes.

-# Settings and open documents are kept in %APPDATA%\tb-notepad. Put a portable.txt next to the exe to keep them in a data folder beside it instead.

-# Get new versions from ⋯ › Get the latest version, and tell us about problems with ⋯ › Report a problem.
