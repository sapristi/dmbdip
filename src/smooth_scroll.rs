/// Smooth scrolling animation state.
///
/// Instead of jumping instantly to the target, the scroll position
/// interpolates toward it over several frames using exponential easing.
/// The target-current gap is capped so that holding a key produces
/// steady scrolling rather than unbounded acceleration.
pub(crate) struct SmoothScroll {
    /// Where we want to end up (in pixels).
    target: f64,
    /// Current interpolated position.
    current: f64,
    /// Whether an animation is in progress.
    pub(crate) active: bool,
}

/// Fraction of remaining distance covered each frame (~60 fps).
const LERP_FACTOR: f64 = 0.25;

/// Maximum distance the target may lead the current position.
/// This prevents unbounded accumulation when holding a key, capping
/// the effective scroll speed.
const MAX_LEAD: f64 = 120.0;

/// Stop animating when we're within this many pixels of the target.
const SNAP_THRESHOLD: f64 = 0.5;

impl SmoothScroll {
    pub(crate) fn new() -> Self {
        SmoothScroll {
            target: 0.0,
            current: 0.0,
            active: false,
        }
    }

    /// Set both current and target to an absolute position (no animation).
    pub(crate) fn jump_to(&mut self, pos: u32) {
        self.current = pos as f64;
        self.target = pos as f64;
        self.active = false;
    }

    /// Request a smooth scroll by a relative delta from the current *target*.
    /// Clamps to [0, max_scroll]. The target-current gap is capped at
    /// MAX_LEAD so holding a key doesn't cause runaway acceleration.
    pub(crate) fn scroll_by(&mut self, delta: i32, max_scroll: u32) {
        let new_target = if delta > 0 {
            (self.target + delta as f64).min(max_scroll as f64)
        } else {
            (self.target + delta as f64).max(0.0)
        };
        // Cap how far the target can lead the current position
        let clamped = new_target.clamp(
            self.current - MAX_LEAD,
            self.current + MAX_LEAD,
        ).clamp(0.0, max_scroll as f64);
        if (clamped - self.target).abs() > 0.01 {
            self.target = clamped;
            self.active = true;
        }
    }

    /// Advance the animation by one frame. Returns the new integer scroll
    /// position, or `None` if nothing changed.
    pub(crate) fn tick(&mut self) -> Option<u32> {
        if !self.active {
            return None;
        }
        let diff = self.target - self.current;
        if diff.abs() < SNAP_THRESHOLD {
            self.current = self.target;
            self.active = false;
        } else {
            self.current += diff * LERP_FACTOR;
        }
        Some(self.current.round() as u32)
    }
}
