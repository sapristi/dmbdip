/// Velocity-based smooth scrolling.
///
/// Each scroll event adds a velocity impulse. Friction decays the
/// velocity each frame. This naturally handles both single taps
/// (short deceleration) and held keys (velocity builds to a steady
/// cruising speed) without the "stall" that target-chasing lerp has
/// between the first press deceleration and key-repeat acceleration.
pub(crate) struct SmoothScroll {
    /// Current scroll position (sub-pixel).
    current: f64,
    /// Current velocity in pixels per frame.
    velocity: f64,
    /// Whether an animation is in progress.
    pub(crate) active: bool,
    /// Upper bound for the scroll position.
    max_scroll: f64,
}

/// Velocity multiplier per frame. Lower = more friction = faster stop.
const FRICTION: f64 = 0.78;

/// Maximum velocity in pixels per frame (~60fps).
/// 65 px/frame ≈ 3900 px/s — fast but controllable.
const MAX_VELOCITY: f64 = 65.0;

/// Stop when velocity drops below this threshold.
const STOP_THRESHOLD: f64 = 0.5;

impl SmoothScroll {
    pub(crate) fn new() -> Self {
        SmoothScroll {
            current: 0.0,
            velocity: 0.0,
            active: false,
            max_scroll: 0.0,
        }
    }

    /// Set position directly with no animation.
    pub(crate) fn jump_to(&mut self, pos: u32) {
        self.current = pos as f64;
        self.velocity = 0.0;
        self.active = false;
    }

    /// Add a scroll impulse. The impulse is scaled so that a single
    /// event (without further input) travels approximately `delta` pixels
    /// before friction stops it.
    pub(crate) fn scroll_by(&mut self, delta: i32, max_scroll: u32) {
        self.max_scroll = max_scroll as f64;
        // Scale so total distance ≈ delta: sum of geometric series
        // impulse / (1 - FRICTION) = delta → impulse = delta * (1 - FRICTION)
        let impulse = delta as f64 * (1.0 - FRICTION);
        self.velocity += impulse;
        self.velocity = self.velocity.clamp(-MAX_VELOCITY, MAX_VELOCITY);
        self.active = true;
    }

    /// Advance one frame. Returns the new scroll position, or `None`
    /// if nothing changed.
    pub(crate) fn tick(&mut self) -> Option<u32> {
        if !self.active {
            return None;
        }
        let prev = self.current.round() as u32;
        self.current += self.velocity;
        self.velocity *= FRICTION;

        // Clamp to bounds
        if self.current < 0.0 {
            self.current = 0.0;
            self.velocity = 0.0;
        } else if self.current > self.max_scroll {
            self.current = self.max_scroll;
            self.velocity = 0.0;
        }

        if self.velocity.abs() < STOP_THRESHOLD {
            self.velocity = 0.0;
            self.active = false;
        }

        let now = self.current.round() as u32;
        if now != prev || self.active {
            Some(now)
        } else {
            self.active = false;
            None
        }
    }
}
