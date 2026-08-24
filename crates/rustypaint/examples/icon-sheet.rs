use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const CELL: f32 = 64.0;
const PAD: f32 = 10.0;
const COLUMNS: usize = 12;

fn main() {
    let res = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../res");
    let groups = [
        ("Chrome", res.join("icons/ui")),
        ("Tools", res.join("icons/tools")),
        ("Lines and curves", res.join("icons/curves")),
        ("Shapes", res.join("shapes")),
        ("The rest", res.clone()),
    ];

    let mut body = String::new();
    let mut y = 0.0;
    for (title, dir) in &groups {
        let files = svgs(dir);
        if files.is_empty() {
            continue;
        }
        for (ground, ink) in [("#f0f2f3", "#1a1a1a"), ("#2a2d33", "#ececee")] {
            let rows = files.len().div_ceil(COLUMNS) as f32;
            let height = rows * CELL + 26.0;
            let _ = write!(
                body,
                r#"  <rect x="0" y="{y}" width="{width}" height="{height}" fill="{ground}"/>
  <text x="8" y="{label}" font-family="sans-serif" font-size="12" fill="{ink}">{title}</text>
"#,
                width = COLUMNS as f32 * CELL,
                label = y + 17.0,
            );

            for (i, file) in files.iter().enumerate() {
                let (column, row) = (i % COLUMNS, i / COLUMNS);
                let inner = std::fs::read_to_string(file).expect("a drawing");
                let inner = inner
                    .lines()
                    .filter(|line| !line.trim_start().starts_with("<?xml"))
                    .collect::<Vec<_>>()
                    .join("\n");
                let _ = writeln!(
                    body,
                    r#"  <g color="{ink}" fill="{ink}" transform="translate({x} {ty})"><svg width="{side}" height="{side}" x="0" y="0">{inner}</svg></g>"#,
                    x = column as f32 * CELL + PAD / 2.0,
                    ty = y + 26.0 + row as f32 * CELL + PAD / 2.0,
                    side = CELL - PAD,
                );
            }
            y += height;
        }
    }

    let width = COLUMNS as f32 * CELL;
    let sheet = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{y}" viewBox="0 0 {width} {y}">
{body}</svg>
"#
    );

    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/icon-sheet.svg");
    std::fs::write(&out, sheet).expect("somewhere to write");
    println!("wrote {}", out.display());
}

fn svgs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "svg"))
        .filter(|p| {
            !p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("icon"))
        })
        .collect();
    out.sort();
    out
}
