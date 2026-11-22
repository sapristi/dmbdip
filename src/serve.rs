//! Headless render server for editor previews (`dmbdip --serve`).
//!
//! Reads line-delimited JSON requests on stdin, renders the given markdown
//! text, writes the page as horizontal strips of raw RGB files that the
//! terminal reads (kitty `t=t`), and answers with one JSON line per request.
//! See `.project/docs/specs/2026-11-22-14-13-nvim-preview-design.md`.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{self, BufRead, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::constants::LayoutParams;
use crate::dlog;
use crate::fonts::Fonts;
use crate::headings::build_headings;
use crate::mermaid::MermaidCache;
use crate::parsing::parse_markdown;
use crate::render::render_markdown;
use crate::theme::Theme;

/// Unicode placeholders carry the row in a diacritic; kitty defines 297 of them.
pub(crate) const MAX_STRIP_ROWS: u32 = 297;
/// Upper bounds that keep a bad request from allocating absurd images.
const MAX_WIDTH_PX: u32 = 20_000;
const MAX_CELL_PX: u32 = 1_000;
/// Rasterized mermaid diagrams kept between requests.
const MERMAID_CACHE_IMAGES: usize = 16;

#[derive(Deserialize)]
struct RenderRequest {
    id: u64,
    text: String,
    cols: u32,
    cell_w: u32,
    cell_h: u32,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub(crate) struct StripInfo {
    pub(crate) index: usize,
    pub(crate) rows: u32,
    pub(crate) path: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct RenderResponse {
    pub(crate) id: u64,
    #[serde(rename = "type")]
    pub(crate) kind: String,
    pub(crate) cols: u32,
    pub(crate) total_rows: u32,
    pub(crate) strips: Vec<StripInfo>,
    pub(crate) anchors: Vec<(usize, u32)>,
}

#[derive(Serialize)]
struct ErrorResponse<'a> {
    id: Option<u64>,
    #[serde(rename = "type")]
    kind: &'static str,
    message: &'a str,
}

/// Where strip files go: `/dev/shm` (RAM) when present, else the temp dir.
pub(crate) fn strip_dir() -> PathBuf {
    let shm = Path::new("/dev/shm");
    if shm.is_dir() {
        shm.to_path_buf()
    } else {
        std::env::temp_dir()
    }
}

struct Server<'a> {
    fonts: &'a Fonts,
    theme: &'a Theme,
    layout: &'a LayoutParams,
    out_dir: &'a Path,
    mermaid_cache: MermaidCache,
    /// `(cols, cell_w, cell_h)` of the last successful render.
    last_geometry: Option<(u32, u32, u32)>,
    /// `(rows, pixel hash)` per strip of the last successful render.
    last_strips: Vec<(u32, u64)>,
    file_counter: u64,
    /// Files written that the terminal may not have consumed yet.
    written: Vec<PathBuf>,
}

impl<'a> Server<'a> {
    fn handle_line(&mut self, line: &str) -> String {
        let value: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return error_json(None, &format!("invalid JSON: {e}")),
        };
        let id = value.get("id").and_then(|v| v.as_u64());
        match value.get("type").and_then(|v| v.as_str()) {
            Some("render") => {}
            Some(other) => return error_json(id, &format!("unknown request type: {other}")),
            None => return error_json(id, "missing request type"),
        }
        let req: RenderRequest = match serde_json::from_value(value) {
            Ok(r) => r,
            Err(e) => return error_json(id, &format!("invalid render request: {e}")),
        };

        let start = Instant::now();
        let result = catch_unwind(AssertUnwindSafe(|| self.render(&req)));
        let json = match result {
            Ok(Ok(resp)) => serde_json::to_string(&resp).expect("response serializes"),
            Ok(Err(msg)) => error_json(id, &msg),
            Err(panic) => {
                let msg = panic
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| panic.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "unknown panic".to_string());
                error_json(id, &format!("render panicked: {msg}"))
            }
        };
        dlog!("serve id={} total_us={}", req.id, start.elapsed().as_micros());
        json
    }

    fn render(&mut self, req: &RenderRequest) -> Result<RenderResponse, String> {
        let (cols, cell_w, cell_h) = (req.cols, req.cell_w, req.cell_h);
        if cols == 0 || cell_w == 0 || cell_h == 0 {
            return Err("cols, cell_w and cell_h must be at least 1".into());
        }
        if cell_w > MAX_CELL_PX || cell_h > MAX_CELL_PX {
            return Err(format!("cell size must be at most {MAX_CELL_PX} px"));
        }
        let width = cols
            .checked_mul(cell_w)
            .filter(|w| *w <= MAX_WIDTH_PX)
            .ok_or_else(|| format!("render width must be at most {MAX_WIDTH_PX} px"))?;
        let min_width = self.layout.margin_left + self.layout.margin_right + self.layout.block_indent;
        if width <= min_width {
            return Err(format!("render width {width} px is too narrow (need more than {min_width} px)"));
        }

        // Forget files the terminal has already read (and deleted).
        self.written.retain(|p| p.exists());

        let (blocks, source_lines) = parse_markdown(&req.text);
        let mut headings = build_headings(&blocks);
        let (img, positions, _) = render_markdown(
            &blocks, &mut headings, width, 0, self.fonts, self.theme, self.layout,
            &mut self.mermaid_cache, false,
        );

        let total_rows = img.height().div_ceil(cell_h).max(1);
        let geometry = (cols, cell_w, cell_h);
        let geometry_changed = self.last_geometry != Some(geometry);
        let raw = img.as_raw();
        let row_bytes = width as usize * 3;
        let bg = self.theme.bg.0;

        let mut strips = Vec::new();
        let mut new_state = Vec::new();
        let mut first_row = 0;
        while first_row < total_rows {
            let rows = (total_rows - first_row).min(MAX_STRIP_ROWS);
            let index = strips.len();
            let bytes = strip_bytes(raw, row_bytes, first_row * cell_h, rows * cell_h, bg);
            let hash = hash_bytes(&bytes);
            let changed = geometry_changed || self.last_strips.get(index) != Some(&(rows, hash));
            let path = if changed {
                Some(self.write_strip(&bytes)?)
            } else {
                None
            };
            strips.push(StripInfo { index, rows, path });
            new_state.push((rows, hash));
            first_row += rows;
        }

        let mut anchors: Vec<(usize, u32)> = positions
            .iter()
            .filter_map(|&(bi, y)| source_lines.get(bi).map(|&line| (line, y / cell_h)))
            .collect();
        anchors.sort_by_key(|&(line, _)| line);

        self.last_geometry = Some(geometry);
        self.last_strips = new_state;
        Ok(RenderResponse { id: req.id, kind: "render".into(), cols, total_rows, strips, anchors })
    }

    fn write_strip(&mut self, bytes: &[u8]) -> Result<String, String> {
        let name = format!(
            "dmbdip-{}-tty-graphics-protocol-{}.rgb",
            std::process::id(),
            self.file_counter,
        );
        self.file_counter += 1;
        let path = self.out_dir.join(name);
        std::fs::write(&path, bytes).map_err(|e| format!("writing {}: {e}", path.display()))?;
        let s = path.to_string_lossy().into_owned();
        self.written.push(path);
        Ok(s)
    }

    fn cleanup(&mut self) {
        for path in self.written.drain(..) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Pixel rows `[y0, y0 + height)` of the image as raw RGB, padding rows past
/// the image's end with the background colour.
fn strip_bytes(raw: &[u8], row_bytes: usize, y0: u32, height: u32, bg: [u8; 3]) -> Vec<u8> {
    let start = (y0 as usize * row_bytes).min(raw.len());
    let end = ((y0 + height) as usize * row_bytes).min(raw.len());
    let mut bytes = Vec::with_capacity(height as usize * row_bytes);
    bytes.extend_from_slice(&raw[start..end]);
    let missing = height as usize * row_bytes - bytes.len();
    bytes.extend(bg.iter().copied().cycle().take(missing));
    bytes
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

fn error_json(id: Option<u64>, message: &str) -> String {
    serde_json::to_string(&ErrorResponse { id, kind: "error", message }).expect("error serializes")
}

/// Serve render requests from `input` until EOF, one JSON response line per
/// request on `output`. Strip files go to `out_dir`; any still present at EOF
/// are removed.
pub(crate) fn run_serve(
    input: impl BufRead,
    mut output: impl Write,
    fonts: &Fonts,
    theme: &Theme,
    layout: &LayoutParams,
    out_dir: &Path,
) -> io::Result<()> {
    let mut server = Server {
        fonts,
        theme,
        layout,
        out_dir,
        mermaid_cache: MermaidCache::with_capacity(MERMAID_CACHE_IMAGES),
        last_geometry: None,
        last_strips: Vec::new(),
        file_counter: 0,
        written: Vec::new(),
    };
    let result = (|| {
        for line in input.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let response = server.handle_line(&line);
            output.write_all(response.as_bytes())?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
        Ok(())
    })();
    server.cleanup();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{test_fonts, SAMPLE_MD};
    use crate::theme::default_theme;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const CELL_W: u32 = 10;
    const CELL_H: u32 = 20;
    const COLS: u32 = 80;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static N: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!(
                "dmbdip-serve-test-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::SeqCst),
            ));
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn files(&self) -> Vec<PathBuf> {
            std::fs::read_dir(&self.0).unwrap().map(|e| e.unwrap().path()).collect()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn long_doc() -> String {
        let mut s = String::new();
        for i in 0..80 {
            s.push_str(&format!("## Section {i}\n\nParagraph {i} with some words to fill the line.\n\n"));
        }
        s
    }

    fn request(id: u64, text: &str, cols: u32) -> String {
        serde_json::json!({
            "id": id, "type": "render", "text": text,
            "cols": cols, "cell_w": CELL_W, "cell_h": CELL_H,
        })
        .to_string()
    }

    /// Run the serve loop over the given request lines; return parsed responses.
    fn serve(lines: &[String], dir: &TempDir) -> Vec<serde_json::Value> {
        let fonts = test_fonts();
        let theme = default_theme();
        let layout = LayoutParams::default();
        let input = lines.join("\n");
        let mut out = Vec::new();
        run_serve(input.as_bytes(), &mut out, &fonts, &theme, &layout, &dir.0).unwrap();
        String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    fn render_resp(v: &serde_json::Value) -> RenderResponse {
        serde_json::from_value(v.clone()).unwrap()
    }

    /// A server that keeps files around so tests can inspect them.
    fn with_server<R>(dir: &TempDir, f: impl FnOnce(&mut Server) -> R) -> R {
        let fonts = test_fonts();
        let theme = default_theme();
        let layout = LayoutParams::default();
        let mut server = Server {
            fonts: &fonts,
            theme: &theme,
            layout: &layout,
            out_dir: &dir.0,
            mermaid_cache: MermaidCache::new(),
            last_geometry: None,
            last_strips: Vec::new(),
            file_counter: 0,
            written: Vec::new(),
        };
        f(&mut server)
    }

    fn render_line(server: &mut Server, line: &str) -> RenderResponse {
        let v: serde_json::Value = serde_json::from_str(&server.handle_line(line)).unwrap();
        assert_eq!(v["type"], "render", "unexpected response: {v}");
        render_resp(&v)
    }

    #[test]
    fn first_render_writes_all_strips_with_exact_size() {
        let dir = TempDir::new();
        let doc = long_doc();
        let resp = with_server(&dir, |s| render_line(s, &request(1, &doc, COLS)));

        assert_eq!(resp.id, 1);
        assert_eq!(resp.cols, COLS);
        assert!(resp.strips.len() >= 2, "expected several strips, got {}", resp.strips.len());
        let sum: u32 = resp.strips.iter().map(|s| s.rows).sum();
        assert_eq!(sum, resp.total_rows);
        for (i, strip) in resp.strips.iter().enumerate() {
            assert_eq!(strip.index, i);
            assert!(strip.rows >= 1 && strip.rows <= MAX_STRIP_ROWS);
            if i + 1 < resp.strips.len() {
                assert_eq!(strip.rows, MAX_STRIP_ROWS);
            }
            let path = strip.path.as_ref().expect("first render sets every path");
            assert!(path.contains("tty-graphics-protocol"));
            let len = std::fs::metadata(path).unwrap().len();
            assert_eq!(len, (COLS * CELL_W * strip.rows * CELL_H * 3) as u64);
        }
    }

    #[test]
    fn padding_rows_use_background() {
        let dir = TempDir::new();
        let resp = with_server(&dir, |s| render_line(s, &request(1, "# Hi\n\nworld", COLS)));
        assert_eq!(resp.strips.len(), 1);
        let bytes = std::fs::read(resp.strips[0].path.as_ref().unwrap()).unwrap();
        let bg = default_theme().bg.0;
        let row_bytes = (COLS * CELL_W * 3) as usize;
        let last_row = &bytes[bytes.len() - row_bytes..];
        assert!(last_row.chunks(3).all(|p| p == bg));
    }

    #[test]
    fn identical_render_sends_nothing() {
        let dir = TempDir::new();
        let doc = long_doc();
        let second = with_server(&dir, |s| {
            render_line(s, &request(1, &doc, COLS));
            render_line(s, &request(2, &doc, COLS))
        });
        assert!(second.strips.iter().all(|s| s.path.is_none()));
    }

    #[test]
    fn edit_near_end_only_resends_last_strip() {
        let dir = TempDir::new();
        let doc = long_doc();
        let edited = format!("{doc}Extra closing words.\n");
        let second = with_server(&dir, |s| {
            render_line(s, &request(1, &doc, COLS));
            render_line(s, &request(2, &edited, COLS))
        });
        assert!(second.strips.len() >= 2);
        assert!(second.strips[0].path.is_none(), "first strip is unchanged");
        assert!(second.strips.last().unwrap().path.is_some(), "last strip changed");
    }

    #[test]
    fn changing_cols_resends_everything() {
        let dir = TempDir::new();
        let doc = long_doc();
        let second = with_server(&dir, |s| {
            render_line(s, &request(1, &doc, COLS));
            render_line(s, &request(2, &doc, COLS + 10))
        });
        assert_eq!(second.cols, COLS + 10);
        assert!(second.strips.iter().all(|s| s.path.is_some()));
    }

    #[test]
    fn anchors_are_sorted_and_monotonic() {
        let dir = TempDir::new();
        let resp = with_server(&dir, |s| render_line(s, &request(1, SAMPLE_MD, COLS)));
        let (blocks, _) = parse_markdown(SAMPLE_MD);
        assert_eq!(resp.anchors.len(), blocks.len());
        for w in resp.anchors.windows(2) {
            assert!(w[0].0 <= w[1].0, "lines sorted: {:?}", w);
            assert!(w[0].1 <= w[1].1, "rows non-decreasing: {:?}", w);
        }
        assert_eq!(resp.anchors[0], (1, LayoutParams::default().paragraph_gap / CELL_H));
        assert!(resp.anchors.last().unwrap().1 < resp.total_rows);
    }

    #[test]
    fn malformed_line_reports_error_and_continues() {
        let dir = TempDir::new();
        let responses = serve(&["not json".to_string(), request(5, "hello", COLS)], &dir);
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0]["type"], "error");
        assert!(responses[0]["id"].is_null());
        assert!(responses[0]["message"].is_string());
        assert_eq!(responses[1]["type"], "render");
        assert_eq!(responses[1]["id"], 5);
    }

    #[test]
    fn invalid_requests_report_errors_with_id() {
        let dir = TempDir::new();
        let responses = serve(
            &[
                r#"{"id": 1, "type": "bogus"}"#.to_string(),
                r#"{"id": 2, "type": "render", "text": "x"}"#.to_string(),
                request(3, "x", 0),
                request(4, "x", 3),
            ],
            &dir,
        );
        assert_eq!(responses.len(), 4);
        for (i, r) in responses.iter().enumerate() {
            assert_eq!(r["type"], "error", "response {i}: {r}");
            assert_eq!(r["id"], i as u64 + 1);
        }
    }

    #[test]
    fn eof_removes_unconsumed_files() {
        let dir = TempDir::new();
        let doc = long_doc();
        let responses = serve(&[request(1, &doc, COLS), request(2, &doc, COLS + 5)], &dir);
        assert_eq!(responses.len(), 2);
        assert!(render_resp(&responses[1]).strips.iter().all(|s| s.path.is_some()));
        assert!(dir.files().is_empty(), "leftover files: {:?}", dir.files());
    }
}
