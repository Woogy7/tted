# TTED editing playground

Start with [`TESTING.md`](TESTING.md) for a guided tour of the current editor.

Use these files to test ordinary editing behavior before syntax highlighting is
implemented.

## Things to try

- Move with arrows, Home, End, Page Up, and Page Down.
- Hold Shift while navigating to select text.
- Click and drag with the mouse.
- Paste several lines at once.
- Undo with `Ctrl+Z` and redo with `Ctrl+Y`.
- Switch tabs with `Ctrl+Tab`.
- Save with `Ctrl+S`.

> A terminal editor should feel unsurprising before it feels clever.

Unicode samples: café · naïve · Ελληνικά · 日本語 · emoji 🚀

```rust
fn main() {
    println!("Hello from a fenced Rust block");
}
```

| Feature | Status |
|---|---|
| Editing | Ready to test |
| Syntax highlighting | Planned |
| LSP | Planned |
