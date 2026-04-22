use pulldown_cmark::{html::push_html, Options, Parser};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let src_root = args
        .next()
        .ok_or_else(|| "Usage: cargo run -- <src_root> [dst_root]".to_string())
        .map(PathBuf::from)?;
    let dst_root = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dst"));

    if args.next().is_some() {
        return Err("Usage: cargo run -- <src_root> [dst_root]".to_string());
    }

    build_site(&src_root, &dst_root).map_err(|err| err.to_string())
}

fn build_site(src_root: &Path, dst_root: &Path) -> Result<(), BuildError> {
    if !src_root.exists() {
        return Err(BuildError::Message(format!(
            "Source root does not exist: {}",
            src_root.display()
        )));
    }
    if !src_root.is_dir() {
        return Err(BuildError::Message(format!(
            "Source root is not a directory: {}",
            src_root.display()
        )));
    }

    let src_name = src_root.file_name().ok_or_else(|| {
        BuildError::Message(format!(
            "Could not determine source folder name for: {}",
            src_root.display()
        ))
    })?;
    let dst_base = dst_root.join(src_name);

    for entry_result in WalkDir::new(src_root) {
        let entry = entry_result.map_err(BuildError::from)?;
        let path = entry.path();

        if !entry.file_type().is_file() {
            continue;
        }

        if path.file_name() == Some(OsStr::new("layout.html")) {
            continue;
        }
        if path.extension() != Some(OsStr::new("md")) {
            continue;
        }

        let rel = path.strip_prefix(src_root).map_err(|err| {
            BuildError::Message(format!("Failed to map {}: {err}", path.display()))
        })?;
        let out_path = dst_base.join(rel).with_extension("html");
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|err| BuildError::IoPath {
                op: "create destination directory",
                path: parent.to_path_buf(),
                source: err,
            })?;
        }

        let markdown = fs::read_to_string(path).map_err(|err| BuildError::IoPath {
            op: "read markdown",
            path: path.to_path_buf(),
            source: err,
        })?;
        let rendered = render_markdown(&markdown);

        let layout = resolve_layout(path, src_root)?;
        let output = apply_layout(&layout, &rendered, path)?;

        fs::write(&out_path, output).map_err(|err| BuildError::IoPath {
            op: "write html output",
            path: out_path.clone(),
            source: err,
        })?;
    }

    Ok(())
}

fn render_markdown(markdown: &str) -> String {
    let parser = Parser::new_ext(markdown, Options::all());
    let mut html = String::new();
    push_html(&mut html, parser);
    html
}

fn resolve_layout(markdown_path: &Path, src_root: &Path) -> Result<String, BuildError> {
    let mut current = markdown_path.parent().ok_or_else(|| {
        BuildError::Message(format!(
            "Markdown path has no parent directory: {}",
            markdown_path.display()
        ))
    })?;

    loop {
        let candidate = current.join("layout.html");
        if candidate.is_file() {
            return fs::read_to_string(&candidate).map_err(|err| BuildError::IoPath {
                op: "read layout",
                path: candidate,
                source: err,
            });
        }

        if current == src_root {
            break;
        }
        current = current.parent().ok_or_else(|| {
            BuildError::Message(format!(
                "Could not walk from {} to src root {} while resolving layout",
                markdown_path.display(),
                src_root.display()
            ))
        })?;
    }

    Err(BuildError::Message(format!(
        "No layout.html found for {}. Searched from {} up to {}",
        markdown_path.display(),
        markdown_path.parent().unwrap_or(markdown_path).display(),
        src_root.display()
    )))
}

fn apply_layout(
    layout: &str,
    rendered_markdown: &str,
    markdown_path: &Path,
) -> Result<String, BuildError> {
    if !layout.contains("{content}") {
        return Err(BuildError::Message(format!(
            "Layout for {} does not contain required token {{content}}",
            markdown_path.display()
        )));
    }
    Ok(layout.replace("{content}", rendered_markdown))
}

#[derive(Debug)]
enum BuildError {
    Message(String),
    IoPath {
        op: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    WalkDir(walkdir::Error),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Message(msg) => write!(f, "{msg}"),
            Self::IoPath { op, path, source } => {
                write!(f, "Failed to {op} at {}: {source}", path.display())
            }
            Self::WalkDir(err) => write!(f, "Directory traversal failed: {err}"),
        }
    }
}

impl std::error::Error for BuildError {}

impl From<walkdir::Error> for BuildError {
    fn from(value: walkdir::Error) -> Self {
        Self::WalkDir(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("recrate_site_builder_{name}_{nonce}"));
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    #[test]
    fn resolve_layout_prefers_closest_parent() {
        let root = temp_dir("closest_layout");
        let src_root = root.join("content");
        let nested = src_root.join("posts/2026");
        fs::create_dir_all(&nested).expect("create nested folder");

        fs::write(src_root.join("layout.html"), "<root>{content}</root>")
            .expect("write root layout");
        fs::create_dir_all(src_root.join("posts")).expect("create posts dir");
        fs::write(
            src_root.join("posts/layout.html"),
            "<posts>{content}</posts>",
        )
        .expect("write posts layout");

        let md_path = nested.join("hello.md");
        fs::write(&md_path, "# hi").expect("write markdown");

        let layout = resolve_layout(&md_path, &src_root).expect("resolve layout");
        assert_eq!(layout, "<posts>{content}</posts>");
    }

    #[test]
    fn map_markdown_path_to_expected_output_location() {
        let src_root = PathBuf::from("/tmp/source");
        let dst_root = PathBuf::from("/tmp/dst");
        let md_path = src_root.join("nested/path/file.md");

        let rel = md_path.strip_prefix(&src_root).expect("strip prefix");
        let out = dst_root.join("source").join(rel).with_extension("html");

        assert_eq!(out, PathBuf::from("/tmp/dst/source/nested/path/file.html"));
    }
}
