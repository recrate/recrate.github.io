# recrate

Static build step:

```bash
cargo run -- <src_root> [dst_root]
```

- Recursively converts `*.md` under `<src_root>` into HTML under `<dst_root>/<src_root_folder_name>/...`.
- Uses the closest `layout.html` found by walking upward from each Markdown file's folder to `<src_root>`.
- Injects rendered Markdown where `{content}` appears in the layout.
- Skips copying any `layout.html` files.
