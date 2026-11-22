//! Headless render server for editor previews (`dmbdip --serve`), protocol 3.
//!
//! Reads line-delimited JSON requests on stdin and answers one JSON line per
//! request. A `layout` request parses and lays out a document and keeps it,
//! with heading slots aligned to cell rows (so the editor can fold whole rows),
//! and answers the rows of every heading and the cursor colour; a
//! `strips` request renders some horizontal strips of the kept document and
//! writes them as raw RGB files that the terminal reads (kitty `t=t`). No
//! full-page image is ever allocated, so memory does not grow with the
//! document's length.
//! See `.project/docs/specs/2026-11-22-15-30-nvim-toggle-large-docs-design.md` and
//! `.project/docs/specs/2026-11-22-17-52-nvim-preview-headings-design.md`.

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
use crate::render::{heading_style, layout_blocks, render_region, BlockLayout};
use crate::text::spans_to_plain;
use crate::theme::Theme;
use crate::types::{Block, HeadingInfo};
use pulldown_cmark::HeadingLevel;

/// Version of the request/response protocol; the plugin refuses others.
pub(crate) const PROTOCOL: u32 = 3;
/// Unicode placeholders carry the row in a diacritic; kitty defines 297 of them.
pub(crate) const MAX_STRIP_ROWS: u32 = 297;
/// Upper bounds that keep a bad request from allocating absurd images.
const MAX_WIDTH_PX: u32 = 20_000;
const MAX_CELL_PX: u32 = 1_000;
/// Rasterized mermaid diagrams kept between requests.
const MERMAID_CACHE_IMAGES: usize = 16;

#[derive(Deserialize)]
struct LayoutRequest {
    id: u64,
    text: String,
    cols: u32,
    cell_w: u32,
    cell_h: u32,
    strip_rows: u32,
}

#[derive(Deserialize)]
struct StripsRequest {
    id: u64,
    doc: u64,
    indices: Vec<u32>,
}

#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct LayoutResponse {
    pub(crate) id: u64,
    #[serde(rename = "type")]
    pub(crate) kind: String,
    pub(crate) protocol: u32,
    pub(crate) doc: u64,
    pub(crate) image_cols: u32,
    pub(crate) offset_cols: u32,
    pub(crate) total_rows: u32,
    pub(crate) strip_rows: u32,
    pub(crate) nstrips: u32,
    pub(crate) bg: String,
    pub(crate) anchors: Vec<(usize, u32)>,
    /// `#rrggbb` of the theme's cursor colour (current-heading bar).
    pub(crate) cursor: String,
    /// Every heading, in document order.
    pub(crate) headings: Vec<HeadingEntry>,
}

/// A heading's rows in the laid-out page. Its slot covers rows
/// `[row, end_row]`; its text starts on `text_row` (after an H1's margin).
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub(crate) struct HeadingEntry {
    pub(crate) level: u8,
    /// 1-based source line.
    pub(crate) line: usize,
    pub(crate) row: u32,
    pub(crate) text_row: u32,
    pub(crate) end_row: u32,
    pub(crate) title: String,
    /// `#rrggbb` of the heading level's colour.
    pub(crate) color: String,
}

fn hex(c: image::Rgb<u8>) -> String {
    let [r, g, b] = c.0;
    format!("#{r:02x}{g:02x}{b:02x}")
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub(crate) struct StripInfo {
    pub(crate) index: u32,
    pub(crate) rows: u32,
    pub(crate) path: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct StripsResponse {
    pub(crate) id: u64,
    #[serde(rename = "type")]
    pub(crate) kind: String,
    pub(crate) protocol: u32,
    pub(crate) doc: u64,
    pub(crate) strips: Vec<StripInfo>,
}

#[derive(Serialize)]
struct StaleResponse {
    id: u64,
    #[serde(rename = "type")]
    kind: &'static str,
    protocol: u32,
    doc: u64,
}

#[derive(Serialize)]
struct ErrorResponse<'a> {
    id: Option<u64>,
    #[serde(rename = "type")]
    kind: &'static str,
    protocol: u32,
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

/// The document of the last successful layout and its geometry.
struct Doc {
    id: u64,
    blocks: Vec<Block>,
    headings: Vec<HeadingInfo>,
    layouts: Vec<BlockLayout>,
    width_px: u32,
    image_cols: u32,
    cell_w: u32,
    cell_h: u32,
    strip_rows: u32,
    total_rows: u32,
    nstrips: u32,
}

struct Server<'a> {
    fonts: &'a Fonts,
    theme: &'a Theme,
    layout: &'a LayoutParams,
    out_dir: &'a Path,
    mermaid_cache: MermaidCache,
    doc: Option<Doc>,
    /// Number of successful layouts so far (the current doc's id).
    doc_counter: u64,
    file_counter: u64,
    /// Files written that the terminal may not have consumed yet.
    written: Vec<PathBuf>,
}

impl<'a> Server<'a> {
    fn new(fonts: &'a Fonts, theme: &'a Theme, layout: &'a LayoutParams, out_dir: &'a Path) -> Self {
        Server {
            fonts,
            theme,
            layout,
            out_dir,
            mermaid_cache: MermaidCache::with_capacity(MERMAID_CACHE_IMAGES),
            doc: None,
            doc_counter: 0,
            file_counter: 0,
            written: Vec::new(),
        }
    }

    fn handle_line(&mut self, line: &str) -> String {
        let value: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return error_json(None, &format!("invalid JSON: {e}")),
        };
        let id = value.get("id").and_then(|v| v.as_u64());
        let kind = match value.get("type").and_then(|v| v.as_str()) {
            Some(k @ ("layout" | "strips")) => k.to_string(),
            Some(other) => return error_json(id, &format!("unknown request type: {other}")),
            None => return error_json(id, "missing request type"),
        };

        let start = Instant::now();
        let result = catch_unwind(AssertUnwindSafe(|| -> Result<String, String> {
            if kind == "layout" {
                let req: LayoutRequest = serde_json::from_value(value)
                    .map_err(|e| format!("invalid layout request: {e}"))?;
                self.layout_doc(&req).map(|r| serde_json::to_string(&r).expect("response serializes"))
            } else {
                let req: StripsRequest = serde_json::from_value(value)
                    .map_err(|e| format!("invalid strips request: {e}"))?;
                self.strips(&req)
            }
        }));
        let json = match result {
            Ok(Ok(json)) => json,
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
        dlog!("serve id={:?} type={} total_us={}", id, kind, start.elapsed().as_micros());
        json
    }

    fn layout_doc(&mut self, req: &LayoutRequest) -> Result<LayoutResponse, String> {
        let (cols, cell_w, cell_h) = (req.cols, req.cell_w, req.cell_h);
        if cols == 0 || cell_w == 0 || cell_h == 0 {
            return Err("cols, cell_w and cell_h must be at least 1".into());
        }
        if cell_w > MAX_CELL_PX || cell_h > MAX_CELL_PX {
            return Err(format!("cell size must be at most {MAX_CELL_PX} px"));
        }
        let l = self.layout;
        let max_width = l.max_content_width + l.margin_left + l.margin_right;
        let width = cols.saturating_mul(cell_w).min(max_width) / cell_w * cell_w;
        if width > MAX_WIDTH_PX {
            return Err(format!("render width must be at most {MAX_WIDTH_PX} px"));
        }
        let min_width = l.margin_left + l.margin_right + l.block_indent;
        if width <= min_width {
            return Err(format!("render width {width} px is too narrow (need more than {min_width} px)"));
        }
        let image_cols = width / cell_w;
        let strip_rows = req.strip_rows.clamp(1, MAX_STRIP_ROWS);

        let (blocks, source_lines) = parse_markdown(&req.text);
        let headings = build_headings(&blocks);
        let (layouts, total_height) = layout_blocks(
            &blocks, &headings, width, self.fonts, self.theme, self.layout, &mut self.mermaid_cache, cell_h,
        );
        let total_rows = total_height.div_ceil(cell_h).max(1);
        let nstrips = total_rows.div_ceil(strip_rows);

        let mut anchors: Vec<(usize, u32)> = layouts
            .iter()
            .filter_map(|b| source_lines.get(b.block_index).map(|&line| (line, b.y / cell_h)))
            .collect();
        anchors.sort_by_key(|&(line, _)| line);

        let headings_out: Vec<HeadingEntry> = layouts
            .iter()
            .filter_map(|b| match &blocks[b.block_index] {
                Block::Heading { level, spans } => {
                    let margin = if matches!(level, HeadingLevel::H1) { l.h1_extra_margin } else { 0 };
                    Some(HeadingEntry {
                        level: *level as u8,
                        line: source_lines.get(b.block_index).copied().unwrap_or(1),
                        row: b.y / cell_h,
                        text_row: (b.y + margin) / cell_h,
                        // Rounded up: the last slot need not end on a row.
                        end_row: (b.y + b.height).div_ceil(cell_h) - 1,
                        title: spans_to_plain(spans),
                        color: hex(heading_style(level, self.theme).1),
                    })
                }
                _ => None,
            })
            .collect();

        self.doc_counter += 1;
        let doc = self.doc_counter;
        self.doc = Some(Doc {
            id: doc,
            blocks,
            headings,
            layouts,
            width_px: width,
            image_cols,
            cell_w,
            cell_h,
            strip_rows,
            total_rows,
            nstrips,
        });
        Ok(LayoutResponse {
            id: req.id,
            kind: "layout".into(),
            protocol: PROTOCOL,
            doc,
            image_cols,
            offset_cols: (cols - image_cols) / 2,
            total_rows,
            strip_rows,
            nstrips,
            bg: hex(self.theme.bg),
            anchors,
            cursor: hex(self.theme.cursor_color),
            headings: headings_out,
        })
    }

    fn strips(&mut self, req: &StripsRequest) -> Result<String, String> {
        let Some(doc) = self.doc.as_mut() else {
            return Err("no document: send a layout request first".into());
        };
        if req.doc != doc.id {
            let stale = StaleResponse { id: req.id, kind: "stale", protocol: PROTOCOL, doc: doc.id };
            return Ok(serde_json::to_string(&stale).expect("stale serializes"));
        }
        if let Some(bad) = req.indices.iter().find(|&&i| i >= doc.nstrips) {
            return Err(format!("strip index {bad} out of range (document has {} strips)", doc.nstrips));
        }

        // Forget files the terminal has already read (and deleted).
        self.written.retain(|p| p.exists());

        let mut strips = Vec::with_capacity(req.indices.len());
        for &index in &req.indices {
            let first_row = index * doc.strip_rows;
            let rows = (doc.total_rows - first_row).min(doc.strip_rows);
            let img = render_region(
                &doc.blocks, &mut doc.headings, doc.width_px, self.fonts, self.theme, self.layout,
                &mut self.mermaid_cache, false, &doc.layouts, first_row * doc.cell_h, rows * doc.cell_h,
            );
            debug_assert_eq!(img.width(), doc.image_cols * doc.cell_w);
            let path = write_strip(self.out_dir, &mut self.file_counter, img.as_raw())?;
            self.written.push(path.clone());
            strips.push(StripInfo { index, rows, path: path.to_string_lossy().into_owned() });
        }
        let resp = StripsResponse { id: req.id, kind: "strips".into(), protocol: PROTOCOL, doc: doc.id, strips };
        Ok(serde_json::to_string(&resp).expect("response serializes"))
    }

    fn cleanup(&mut self) {
        for path in self.written.drain(..) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Write raw RGB bytes to a new strip file. The name must contain
/// `tty-graphics-protocol`: kitty only deletes temp files that do.
fn write_strip(out_dir: &Path, counter: &mut u64, bytes: &[u8]) -> Result<PathBuf, String> {
    let name = format!("dmbdip-{}-tty-graphics-protocol-{}.rgb", std::process::id(), counter);
    *counter += 1;
    let path = out_dir.join(name);
    std::fs::write(&path, bytes).map_err(|e| format!("writing {}: {e}", path.display()))?;
    Ok(path)
}

fn error_json(id: Option<u64>, message: &str) -> String {
    serde_json::to_string(&ErrorResponse { id, kind: "error", protocol: PROTOCOL, message })
        .expect("error serializes")
}

/// Serve requests from `input` until EOF, one JSON response line per request
/// on `output`. Strip files go to `out_dir`; any still present at EOF are
/// removed.
pub(crate) fn run_serve(
    input: impl BufRead,
    mut output: impl Write,
    fonts: &Fonts,
    theme: &Theme,
    layout: &LayoutParams,
    out_dir: &Path,
) -> io::Result<()> {
    let mut server = Server::new(fonts, theme, layout, out_dir);
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

    fn layout_req(id: u64, text: &str, cols: u32, strip_rows: u32) -> String {
        serde_json::json!({
            "id": id, "type": "layout", "text": text,
            "cols": cols, "cell_w": CELL_W, "cell_h": CELL_H, "strip_rows": strip_rows,
        })
        .to_string()
    }

    fn strips_req(id: u64, doc: u64, indices: &[u32]) -> String {
        serde_json::json!({ "id": id, "type": "strips", "doc": doc, "indices": indices }).to_string()
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

    /// A server that keeps files around so tests can inspect them.
    fn with_server<R>(dir: &TempDir, f: impl FnOnce(&mut Server) -> R) -> R {
        let fonts = test_fonts();
        let theme = default_theme();
        let layout = LayoutParams::default();
        let mut server = Server::new(&fonts, &theme, &layout, &dir.0);
        f(&mut server)
    }

    fn call(server: &mut Server, line: &str) -> serde_json::Value {
        let v: serde_json::Value = serde_json::from_str(&server.handle_line(line)).unwrap();
        assert_eq!(v["protocol"], PROTOCOL, "every response carries the protocol: {v}");
        v
    }

    fn layout(server: &mut Server, line: &str) -> LayoutResponse {
        let v = call(server, line);
        assert_eq!(v["type"], "layout", "unexpected response: {v}");
        serde_json::from_value(v).unwrap()
    }

    fn strips(server: &mut Server, line: &str) -> StripsResponse {
        let v = call(server, line);
        assert_eq!(v["type"], "strips", "unexpected response: {v}");
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn wide_window_caps_image_and_centres_it() {
        let dir = TempDir::new();
        let resp = with_server(&dir, |s| layout(s, &layout_req(1, "# Hi\n\nworld", 200, 10)));
        let l = LayoutParams::default();
        let max = l.max_content_width + l.margin_left + l.margin_right;
        assert_eq!(resp.image_cols, max / CELL_W);
        assert_eq!(resp.offset_cols, (200 - resp.image_cols) / 2);
        assert!(resp.offset_cols > 0);
        assert_eq!(resp.doc, 1);
        let [r, g, b] = default_theme().bg.0;
        assert_eq!(resp.bg, format!("#{r:02x}{g:02x}{b:02x}"));
    }

    #[test]
    fn narrow_window_uses_full_width() {
        let dir = TempDir::new();
        let resp = with_server(&dir, |s| layout(s, &layout_req(1, "# Hi\n\nworld", 45, 10)));
        assert_eq!(resp.image_cols, 45);
        assert_eq!(resp.offset_cols, 0);
    }

    #[test]
    fn strip_count_and_clamping() {
        let dir = TempDir::new();
        let doc = long_doc();
        with_server(&dir, |s| {
            let r = layout(s, &layout_req(1, &doc, 80, 48));
            assert_eq!(r.strip_rows, 48);
            assert_eq!(r.nstrips, r.total_rows.div_ceil(48));
            assert!(r.nstrips > 3);
            let r = layout(s, &layout_req(2, &doc, 80, 0));
            assert_eq!((r.strip_rows, r.nstrips), (1, r.total_rows));
            let r = layout(s, &layout_req(3, &doc, 80, 1000));
            assert_eq!(r.strip_rows, MAX_STRIP_ROWS);
            assert_eq!(r.doc, 3, "doc increments on each layout");
        });
    }

    #[test]
    fn layout_writes_no_files() {
        let dir = TempDir::new();
        with_server(&dir, |s| layout(s, &layout_req(1, &long_doc(), 80, 48)));
        assert!(dir.files().is_empty());
    }

    #[test]
    fn strips_have_exact_size_and_stitch_to_full_render() {
        let dir = TempDir::new();
        let cols = 80;
        let (resp, files) = with_server(&dir, |s| {
            let l = layout(s, &layout_req(1, SAMPLE_MD, cols, 13));
            let all: Vec<u32> = (0..l.nstrips).collect();
            let r = strips(s, &strips_req(2, l.doc, &all));
            let files: Vec<Vec<u8>> = r.strips.iter().map(|st| std::fs::read(&st.path).unwrap()).collect();
            (l, (r, files))
        });
        let (sresp, files) = files;
        assert_eq!(sresp.doc, 1);
        assert_eq!(sresp.strips.len() as u32, resp.nstrips);
        let width = resp.image_cols * CELL_W;
        let mut stitched = Vec::new();
        for (i, (st, bytes)) in sresp.strips.iter().zip(&files).enumerate() {
            assert_eq!(st.index, i as u32);
            assert!(st.path.contains("tty-graphics-protocol"));
            let want_rows = if i as u32 + 1 < resp.nstrips { 13 } else { resp.total_rows - 13 * i as u32 };
            assert_eq!(st.rows, want_rows);
            assert_eq!(bytes.len(), (width * st.rows * CELL_H * 3) as usize);
            stitched.extend_from_slice(bytes);
        }

        // The full page drawn from the same row-aligned layout.
        let fonts = test_fonts();
        let theme = default_theme();
        let params = LayoutParams::default();
        let (blocks, _) = parse_markdown(SAMPLE_MD);
        let mut headings = build_headings(&blocks);
        let mut cache = MermaidCache::new();
        let (layouts, total) = layout_blocks(&blocks, &headings, width, &fonts, &theme, &params, &mut cache, CELL_H);
        let full = render_region(&blocks, &mut headings, width, &fonts, &theme, &params, &mut cache, false, &layouts, 0, total);
        let raw = full.as_raw();
        assert_eq!(&stitched[..raw.len()], &raw[..], "strips equal the full render");
        assert!(stitched[raw.len()..].chunks(3).all(|p| p == theme.bg.0), "padding is bg");
    }

    #[test]
    fn anchors_are_sorted_and_monotonic() {
        let dir = TempDir::new();
        let resp = with_server(&dir, |s| layout(s, &layout_req(1, SAMPLE_MD, 80, 50)));
        let (blocks, _) = parse_markdown(SAMPLE_MD);
        assert_eq!(resp.anchors.len(), blocks.len());
        for w in resp.anchors.windows(2) {
            assert!(w[0].0 <= w[1].0, "lines sorted: {:?}", w);
            assert!(w[0].1 <= w[1].1, "rows non-decreasing: {:?}", w);
        }
        // SAMPLE_MD starts with a heading, whose slot is aligned to a row.
        assert_eq!(resp.anchors[0], (1, LayoutParams::default().paragraph_gap.div_ceil(CELL_H)));
        assert!(resp.anchors.last().unwrap().1 < resp.total_rows);
    }

    #[test]
    fn layout_reports_heading_rows_and_cursor() {
        let dir = TempDir::new();
        let md = "# A\n\nText a.\n\n## A1\n\nText a1.\n\n### A1a\n\nText a1a.\n\n## A2\n\nText a2.\n\n# B\n\nText b.\n";
        let resp = with_server(&dir, |s| layout(s, &layout_req(1, md, 80, 50)));
        assert_eq!(PROTOCOL, 3);
        let theme = default_theme();
        assert_eq!(resp.cursor, hex(theme.cursor_color));
        let got: Vec<(u8, &str, usize)> = resp.headings.iter().map(|h| (h.level, h.title.as_str(), h.line)).collect();
        assert_eq!(got, vec![(1, "A", 1), (2, "A1", 5), (3, "A1a", 9), (2, "A2", 13), (1, "B", 17)]);
        assert_eq!(resp.headings[0].color, hex(theme.h1_color));
        assert_eq!(resp.headings[1].color, hex(theme.h2_color));
        let h1 = &resp.headings[0];
        assert_eq!(h1.text_row, (h1.row * CELL_H + LayoutParams::default().h1_extra_margin) / CELL_H, "H1 text after its margin");
        for h in &resp.headings {
            assert!(h.row <= h.text_row && h.text_row <= h.end_row, "{h:?}");
            assert!(h.end_row < resp.total_rows);
            assert!(resp.anchors.contains(&(h.line, h.row)), "row of {h:?} is its anchor row");
        }
        for w in resp.headings.windows(2) {
            assert!(w[0].row < w[1].row, "rows increase: {w:?}");
            assert!(w[0].end_row < w[1].row, "slots don't overlap: {w:?}");
        }
    }

    #[test]
    fn stale_doc_and_bad_indices() {
        let dir = TempDir::new();
        with_server(&dir, |s| {
            let v = call(s, &strips_req(1, 1, &[0]));
            assert_eq!(v["type"], "error", "strips before layout: {v}");
            layout(s, &layout_req(2, "hello", 80, 10));
            let l = layout(s, &layout_req(3, "hello again", 80, 10));
            let v = call(s, &strips_req(4, l.doc - 1, &[0]));
            assert_eq!(v["type"], "stale");
            assert_eq!(v["id"], 4);
            assert_eq!(v["doc"], l.doc);
            let v = call(s, &strips_req(5, l.doc, &[l.nstrips]));
            assert_eq!(v["type"], "error");
            assert_eq!(v["id"], 5);
        });
        assert!(dir.files().is_empty(), "no files for stale or bad requests");
    }

    #[test]
    fn malformed_line_reports_error_and_continues() {
        let dir = TempDir::new();
        let responses = serve(&["not json".to_string(), layout_req(5, "hello", 80, 10)], &dir);
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0]["type"], "error");
        assert_eq!(responses[0]["protocol"], PROTOCOL);
        assert!(responses[0]["id"].is_null());
        assert!(responses[0]["message"].is_string());
        assert_eq!(responses[1]["type"], "layout");
        assert_eq!(responses[1]["id"], 5);
    }

    #[test]
    fn invalid_requests_report_errors_with_id() {
        let dir = TempDir::new();
        let responses = serve(
            &[
                r#"{"id": 1, "type": "bogus"}"#.to_string(),
                r#"{"id": 2, "type": "layout", "text": "x"}"#.to_string(),
                layout_req(3, "x", 0, 10),
                layout_req(4, "x", 3, 10),
                r#"{"id": 5, "type": "render", "text": "x", "cols": 80, "cell_w": 10, "cell_h": 20}"#.to_string(),
            ],
            &dir,
        );
        assert_eq!(responses.len(), 5);
        for (i, r) in responses.iter().enumerate() {
            assert_eq!(r["type"], "error", "response {i}: {r}");
            assert_eq!(r["id"], i as u64 + 1);
        }
    }

    #[test]
    fn eof_removes_unconsumed_files() {
        let dir = TempDir::new();
        let responses = serve(&[layout_req(1, &long_doc(), 80, 48), strips_req(2, 1, &[0, 1, 2])], &dir);
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[1]["strips"].as_array().unwrap().len(), 3);
        assert!(dir.files().is_empty(), "leftover files: {:?}", dir.files());
    }
}
