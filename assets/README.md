# CLI banner

`banner.txt` spells `kicker.rs` in Small Slant. Generate or check it with the
`banner` operation of the verified standards engine fixed in
[standards/engine.json](../standards/engine.json). The engine owns the renderer,
pinned font data and canonical spacing/newline rules.

Rust embeds the generated ASCII text with `include_str!`. Product build and
runtime need no font, Node.js or FIGlet implementation.

Font attribution: [SmSlant by Glenn Chappell](https://github.com/cmatsuoka/figlet/blob/master/fonts/smslant.flf).
The font itself is not modified or vendored here.
