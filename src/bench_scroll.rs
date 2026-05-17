//! Off-terminal scroll benchmark. Writes to a `Vec<u8>` sink, so the
//! `write` timings here measure memcpy speed, not actual terminal drain.
//! Per-frame breakdown still shows copy / highlight / base64 costs.
#![cfg(test)]

use crate::constants::LayoutParams;
use crate::kitty::display_viewport;
use crate::state::AppState;
use crate::test_helpers::test_fonts;
use crate::theme::default_theme;
use std::time::Instant;

fn run_bench(label: &str, content: &str, vp_w: u32, vp_h: u32) {
    let fonts = test_fonts();
    let theme = default_theme();
    let layout = LayoutParams::default();
    let mut state = AppState::new(content, &fonts, vp_w, vp_h, theme, layout);

    eprintln!(
        "\n=== {label} ===\n  doc image: {}x{} ({} MB)  max_scroll={}",
        state.img.width(),
        state.img.height(),
        (state.img.width() as usize * state.img.height() as usize * 3) / (1024 * 1024),
        state.max_scroll()
    );

    let mut sink: Vec<u8> = Vec::with_capacity(16 * 1024 * 1024);
    let max = state.max_scroll();
    let step = 40u32;
    let n_frames = ((max / step.max(1)).max(1) as usize).min(200);

    let mut frames_us: Vec<u128> = Vec::with_capacity(n_frames);
    for i in 0..n_frames {
        sink.clear();
        state.scroll_y = ((i as u32) * step).min(max);
        let ci = state.cursor_info();
        let t = Instant::now();
        display_viewport(
            &mut sink,
            &state.img,
            state.scroll_y,
            vp_w,
            vp_h,
            &mut state.frame,
            None,
            None,
            ci,
            &state.search_highlights,
            state.search_current,
        )
        .unwrap();
        frames_us.push(t.elapsed().as_micros());
    }

    frames_us.sort();
    let n = frames_us.len();
    let avg: u128 = frames_us.iter().sum::<u128>() / n as u128;
    let median = frames_us[n / 2];
    let p95 = frames_us[(n * 95 / 100).min(n - 1)];
    let max_f = *frames_us.last().unwrap();
    eprintln!(
        "  frames: n={n} avg={avg}us median={median}us p95={p95}us max={max_f}us  bytes/frame≈{}",
        sink.len()
    );
}

#[test]
fn bench_scroll_sample_and_large() {
    // Initialize the timing log so dlog! lines from display_viewport land in
    // /tmp/dmbdip-bench.log. Only the first call wins (OnceLock), so this is safe.
    unsafe { std::env::set_var("DMBDIP_DEBUG_LOG", "/tmp/dmbdip-bench.log"); }
    let _ = crate::debug::init(true);

    let sample = std::fs::read_to_string("docs/sample.md").expect("read sample.md");

    // 1) small typical viewport
    run_bench("sample.md @ 1200x800", &sample, 1200, 800);
    // 2) large viewport (1080p-ish)
    run_bench("sample.md @ 1920x1080", &sample, 1920, 1080);
    // 3) long doc (sample × 40) @ 1080p — exaggerates doc image size
    let long = sample.repeat(40);
    run_bench("sample × 40 @ 1920x1080", &long, 1920, 1080);
}
