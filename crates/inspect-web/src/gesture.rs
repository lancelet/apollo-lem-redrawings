//! Trackpad-first input (docs/redraw-plan.md §5, "Trackpad first"): what a
//! wheel event or a Safari gesture means. Pure, so it is tested natively;
//! the chrome turns the result into the inspector's `Pan` and `Zoom`.

//!
//! | input | meaning |
//! |---|---|
//! | wheel with `ctrlKey` (a pinch in Chrome, Edge and Firefox) | zoom about the cursor, fine rate |
//! | wheel with `metaKey` (⌘ + scroll) | zoom about the cursor |
//! | Safari `gesturechange` (pinch) | zoom about the cursor by the change in `scale` |
//! | any other wheel from a trackpad | two-finger scroll: pan |
//! | any other wheel from a mouse | zoom (its notches are the only motion it has) |
//!
//! Whether a plain wheel event is a mouse is a guess unless the user says
//! ([`WheelMode`]): `auto` takes `deltaMode` line or page, or a large whole
//! number of pixels with no sideways part (a notch: 100 or 120 on most
//! systems), as a mouse wheel; a trackpad sends small and fractional steps.
//!
//! Touch (the mobile layout) is here too: [`Touches`] turns fingers going
//! down, moving and lifting into pan, pinch, tap and long press, and
//! [`sheet_after_drag`] says what a drag on the bottom sheet's grip does.

use lmsheets_inspector::Sheet;

/// What a plain wheel (no ctrl, no ⌘) does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WheelMode {
    /// Guess from the event: a mouse wheel zooms, a trackpad scroll pans.
    #[default]
    Auto,
    /// Every plain wheel zooms.
    WheelZooms,
}

impl WheelMode {
    /// The other setting.
    pub fn toggled(self) -> Self {
        match self {
            WheelMode::Auto => WheelMode::WheelZooms,
            WheelMode::WheelZooms => WheelMode::Auto,
        }
    }

    /// The setting's label.
    pub fn label(self) -> &'static str {
        match self {
            WheelMode::Auto => "auto",
            WheelMode::WheelZooms => "wheel zooms",
        }
    }
}

/// A wheel event, as the page reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wheel {
    /// `deltaX`.
    pub dx: f64,
    /// `deltaY`.
    pub dy: f64,
    /// `deltaMode`: 0 pixels, 1 lines, 2 pages.
    pub mode: u32,
    /// `ctrlKey` (also set by Chrome for a trackpad pinch).
    pub ctrl: bool,
    /// `metaKey` (⌘).
    pub meta: bool,
}

/// What an input asks of the view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gesture {
    /// Move the content by this many CSS px.
    Pan((f64, f64)),
    /// Zoom by this factor about the cursor.
    Zoom(f64),
}

/// Zoom per pixel of a pinch's `deltaY` (Chrome sends small steps, a few px
/// each, at the fingers' pace): about 2.7 over 100 px.
const PINCH_RATE: f64 = 0.01;
/// Zoom per pixel of ⌘ + scroll.
const META_RATE: f64 = 0.005;
/// Zoom per pixel of a mouse wheel: a 100 px notch is about 16 %.
const WHEEL_RATE: f64 = 0.0015;
/// A pinch or ⌘ step never zooms by more than this exponent in one event, so
/// a mouse's ctrl + wheel notch is a step, not a jump.
const MAX_STEP: f64 = 0.4;
/// Pixels of a line and of a page, for `deltaMode` 1 and 2.
const LINE_PX: f64 = 16.0;
const PAGE_PX: f64 = 400.0;
/// The least whole-number pixel step taken for a mouse wheel notch.
const NOTCH_PX: f64 = 50.0;

fn pixels(w: &Wheel) -> (f64, f64) {
    let k = match w.mode {
        1 => LINE_PX,
        2 => PAGE_PX,
        _ => 1.0,
    };
    (w.dx * k, w.dy * k)
}

/// Whether a plain wheel event looks like a mouse wheel.
pub fn looks_like_mouse(w: &Wheel) -> bool {
    w.mode != 0 || (w.dx == 0.0 && w.dy.fract() == 0.0 && w.dy.abs() >= NOTCH_PX)
}

fn zoom_by(dy_px: f64, rate: f64) -> Gesture {
    Gesture::Zoom((-(dy_px * rate).clamp(-MAX_STEP, MAX_STEP)).exp())
}

/// What a wheel event means under `mode`.
pub fn wheel(w: Wheel, mode: WheelMode) -> Gesture {
    let (dx, dy) = pixels(&w);
    if w.ctrl {
        return zoom_by(dy, PINCH_RATE);
    }
    if w.meta {
        return zoom_by(if dy != 0.0 { dy } else { dx }, META_RATE);
    }
    if mode == WheelMode::WheelZooms || looks_like_mouse(&w) {
        return zoom_by(dy, WHEEL_RATE);
    }
    Gesture::Pan((-dx, -dy))
}

/// Safari's pinch: the factor for a `gesturechange` whose cumulative
/// `scale` is `scale`, the last one having been `last` (1 at the start).
/// Non-finite or non-positive scales change nothing.
pub fn gesture_factor(scale: f64, last: f64) -> f64 {
    if scale.is_finite() && last.is_finite() && scale > 0.0 && last > 0.0 {
        scale / last
    } else {
        1.0
    }
}

/// How far (CSS px) a drag on the bottom sheet's grip must go to change it.
const SHEET_DRAG: f64 = 40.0;

/// The bottom sheet after a drag on its grip by `dy` CSS px (negative is up):
/// up raises it one step (summary, half, full), down lowers it, and a tap on
/// the summary opens it half. `Closed` stays closed.
pub fn sheet_after_drag(now: Sheet, dy: f64) -> Sheet {
    use Sheet::*;
    if now == Closed {
        return Closed;
    }
    if dy <= -SHEET_DRAG {
        match now {
            Peek => Half,
            _ => Full,
        }
    } else if dy >= SHEET_DRAG {
        match now {
            Full => Half,
            _ => Peek,
        }
    } else if dy.abs() < TAP_SLOP && now == Peek {
        Half
    } else {
        now
    }
}

/// A finger that moves less than this (CSS px) has not moved: a tap.
const TAP_SLOP: f64 = 8.0;
/// A tap is shorter than this, ms.
const TAP_MS: f64 = 450.0;
/// A finger held still this long is a long press, ms.
const LONG_MS: f64 = 500.0;

/// What touch input asks of the view. Positions are CSS px in the pane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TouchAction {
    /// Move the content by this much (one finger, or two fingers' centre).
    Pan((f64, f64)),
    /// Zoom by this factor about the point (two fingers' centre).
    Zoom {
        /// The factor, greater than 1 zooms in.
        factor: f64,
        /// The fixed point.
        about: (f64, f64),
    },
    /// A quick touch that stayed put.
    Tap((f64, f64)),
    /// A touch held still.
    LongPress((f64, f64)),
}

#[derive(Clone, Copy, Debug)]
struct Finger {
    id: i32,
    start: (f64, f64),
    at: (f64, f64),
    t0: f64,
}

/// Touch input as fingers go down, move and lift: one finger pans, two pan
/// and pinch, a short still touch is a tap, a long still touch a long press.
/// Pure (the clock is passed in), so it is tested natively.
#[derive(Clone, Debug, Default)]
pub struct Touches {
    fingers: Vec<Finger>,
    /// Whether this gesture has moved past the slop, or had two fingers:
    /// it can no longer be a tap or a long press.
    moved: bool,
    long_fired: bool,
}

fn centre(f: &[Finger]) -> (f64, f64) {
    let n = f.len() as f64;
    (
        f.iter().map(|p| p.at.0).sum::<f64>() / n,
        f.iter().map(|p| p.at.1).sum::<f64>() / n,
    )
}

fn spread(f: &[Finger]) -> f64 {
    (f[0].at.0 - f[1].at.0).hypot(f[0].at.1 - f[1].at.1)
}

impl Touches {
    /// A finger went down at `at`, `t` ms.
    pub fn down(&mut self, id: i32, at: (f64, f64), t: f64) {
        if self.fingers.is_empty() {
            self.moved = false;
            self.long_fired = false;
        } else {
            self.moved = true;
        }
        self.fingers.retain(|f| f.id != id);
        self.fingers.push(Finger {
            id,
            start: at,
            at,
            t0: t,
        });
    }

    /// Whether any finger is down.
    #[cfg(test)]
    pub fn active(&self) -> bool {
        !self.fingers.is_empty()
    }

    /// A finger moved to `at`.
    pub fn moved(&mut self, id: i32, at: (f64, f64)) -> Vec<TouchAction> {
        let Some(i) = self.fingers.iter().position(|f| f.id == id) else {
            return Vec::new();
        };
        let before = self.fingers.clone();
        self.fingers[i].at = at;
        let mut out = Vec::new();
        match self.fingers.len() {
            1 => {
                let f = self.fingers[0];
                if !self.moved {
                    if (f.at.0 - f.start.0).hypot(f.at.1 - f.start.1) < TAP_SLOP {
                        return out;
                    }
                    self.moved = true;
                    // The pan from the start, so the slop is not lost.
                    out.push(TouchAction::Pan((f.at.0 - f.start.0, f.at.1 - f.start.1)));
                } else {
                    out.push(TouchAction::Pan((
                        at.0 - before[0].at.0,
                        at.1 - before[0].at.1,
                    )));
                }
            }
            2 => {
                self.moved = true;
                let (c0, c1) = (centre(&before), centre(&self.fingers));
                let (d0, d1) = (spread(&before), spread(&self.fingers));
                out.push(TouchAction::Pan((c1.0 - c0.0, c1.1 - c0.1)));
                if d0 > 1.0 && d1 > 1.0 {
                    out.push(TouchAction::Zoom {
                        factor: d1 / d0,
                        about: c1,
                    });
                }
            }
            _ => {}
        }
        out
    }

    /// A finger lifted at `t` ms: a tap if it was the only touch, quick and
    /// still.
    pub fn up(&mut self, id: i32, t: f64) -> Vec<TouchAction> {
        let Some(i) = self.fingers.iter().position(|f| f.id == id) else {
            return Vec::new();
        };
        let f = self.fingers.remove(i);
        let tap = self.fingers.is_empty() && !self.moved && !self.long_fired && t - f.t0 < TAP_MS;
        if self.fingers.is_empty() {
            self.moved = false;
            self.long_fired = false;
        }
        if tap {
            vec![TouchAction::Tap(f.at)]
        } else {
            Vec::new()
        }
    }

    /// All touches were cancelled.
    pub fn cancel(&mut self) {
        self.fingers.clear();
        self.moved = false;
        self.long_fired = false;
    }

    /// Called every frame with the clock: a long press once, when one finger
    /// has been still for [`LONG_MS`].
    pub fn poll(&mut self, t: f64) -> Vec<TouchAction> {
        if let [f] = self.fingers[..] {
            if !self.moved && !self.long_fired && t - f.t0 >= LONG_MS {
                self.long_fired = true;
                return vec![TouchAction::LongPress(f.at)];
            }
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(dx: f64, dy: f64) -> Wheel {
        Wheel {
            dx,
            dy,
            mode: 0,
            ctrl: false,
            meta: false,
        }
    }

    #[test]
    fn a_two_finger_scroll_pans_the_content_the_way_the_fingers_go() {
        // Scrolling down (positive deltaY) moves the content up.
        assert_eq!(
            wheel(w(3.5, 12.25), WheelMode::Auto),
            Gesture::Pan((-3.5, -12.25))
        );
        // Small whole steps are a trackpad too.
        assert_eq!(
            wheel(w(0.0, 7.0), WheelMode::Auto),
            Gesture::Pan((0.0, -7.0))
        );
        // A diagonal scroll with a large whole dy still has a sideways part.
        assert!(matches!(
            wheel(w(2.0, 120.0), WheelMode::Auto),
            Gesture::Pan(_)
        ));
    }

    #[test]
    fn a_pinch_zooms_about_the_cursor_at_a_fine_rate_and_either_way() {
        let pinch = |dy| Wheel {
            ctrl: true,
            ..w(0.0, dy)
        };
        let (Gesture::Zoom(out), Gesture::Zoom(inn)) = (
            wheel(pinch(4.0), WheelMode::Auto),
            wheel(pinch(-4.0), WheelMode::Auto),
        ) else {
            panic!("pinch zooms")
        };
        assert!(out < 1.0 && inn > 1.0);
        assert!((out * inn - 1.0).abs() < 1e-12);
        // A few px is a few percent, not a jump.
        assert!((inn - 1.0).abs() < 0.06, "{inn}");
        // A mouse's ctrl + notch is bounded.
        let Gesture::Zoom(notch) = wheel(pinch(-120.0), WheelMode::Auto) else {
            panic!()
        };
        assert!(notch <= MAX_STEP.exp() + 1e-12);
        // A pinch never pans, even when the mouse setting is on.
        assert!(matches!(
            wheel(pinch(1.0), WheelMode::WheelZooms),
            Gesture::Zoom(_)
        ));
    }

    #[test]
    fn command_scroll_zooms() {
        let m = Wheel {
            meta: true,
            ..w(0.0, -10.0)
        };
        let Gesture::Zoom(f) = wheel(m, WheelMode::Auto) else {
            panic!()
        };
        assert!(f > 1.0 && f < 1.1);
    }

    #[test]
    fn auto_takes_lines_and_large_whole_steps_for_a_mouse_wheel() {
        let line = Wheel {
            mode: 1,
            ..w(0.0, 3.0)
        };
        assert!(matches!(wheel(line, WheelMode::Auto), Gesture::Zoom(f) if f < 1.0));
        assert!(matches!(wheel(w(0.0, -100.0), WheelMode::Auto), Gesture::Zoom(f) if f > 1.0));
        assert!(matches!(wheel(w(0.0, 120.0), WheelMode::Auto), Gesture::Zoom(f) if f < 1.0));
        // Fractional large steps are a trackpad flick.
        assert!(matches!(
            wheel(w(0.0, 120.5), WheelMode::Auto),
            Gesture::Pan(_)
        ));
    }

    #[test]
    fn the_setting_makes_every_plain_wheel_zoom() {
        assert!(matches!(
            wheel(w(0.0, 7.0), WheelMode::WheelZooms),
            Gesture::Zoom(_)
        ));
        assert_eq!(WheelMode::Auto.toggled(), WheelMode::WheelZooms);
        assert_eq!(WheelMode::WheelZooms.toggled(), WheelMode::Auto);
    }

    #[test]
    fn a_safari_pinch_zooms_by_the_change_in_scale() {
        assert!((gesture_factor(1.2, 1.0) - 1.2).abs() < 1e-12);
        assert!((gesture_factor(1.5, 1.2) - 1.25).abs() < 1e-12);
        assert_eq!(gesture_factor(0.0, 1.0), 1.0);
        assert_eq!(gesture_factor(f64::NAN, 1.0), 1.0);
    }

    #[test]
    fn dragging_the_sheet_steps_it_up_and_down() {
        use Sheet::*;
        assert_eq!(sheet_after_drag(Peek, -60.0), Half);
        assert_eq!(sheet_after_drag(Half, -60.0), Full);
        assert_eq!(sheet_after_drag(Full, -60.0), Full);
        assert_eq!(sheet_after_drag(Full, 60.0), Half);
        assert_eq!(sheet_after_drag(Half, 60.0), Peek);
        assert_eq!(sheet_after_drag(Peek, 60.0), Peek);
        assert_eq!(sheet_after_drag(Peek, 2.0), Half, "a tap on the summary");
        assert_eq!(sheet_after_drag(Half, 2.0), Half);
        assert_eq!(
            sheet_after_drag(Half, -20.0),
            Half,
            "a short drag is nothing"
        );
        assert_eq!(sheet_after_drag(Closed, -90.0), Closed);
    }

    #[test]
    fn a_quick_still_touch_is_a_tap_and_a_drag_is_not() {
        let mut t = Touches::default();
        t.down(1, (50.0, 60.0), 0.0);
        assert!(t.moved(1, (53.0, 62.0)).is_empty(), "within the slop");
        assert_eq!(t.up(1, 120.0), [TouchAction::Tap((53.0, 62.0))]);
        assert!(!t.active());
        // Past the slop it pans (from the start), then by each step; no tap.
        t.down(1, (50.0, 60.0), 0.0);
        assert_eq!(t.moved(1, (70.0, 60.0)), [TouchAction::Pan((20.0, 0.0))]);
        assert_eq!(t.moved(1, (75.0, 64.0)), [TouchAction::Pan((5.0, 4.0))]);
        assert!(t.up(1, 100.0).is_empty());
        // Too slow is not a tap.
        t.down(1, (0.0, 0.0), 0.0);
        assert!(t.up(1, 900.0).is_empty());
    }

    #[test]
    fn two_fingers_pan_by_their_centre_and_zoom_by_their_spread() {
        let mut t = Touches::default();
        t.down(1, (100.0, 100.0), 0.0);
        t.down(2, (200.0, 100.0), 10.0);
        // Spread from 100 to 150 about the new centre; the centre moved 25.
        let a = t.moved(2, (250.0, 100.0));
        assert_eq!(a[0], TouchAction::Pan((25.0, 0.0)));
        assert_eq!(
            a[1],
            TouchAction::Zoom {
                factor: 1.5,
                about: (175.0, 100.0)
            }
        );
        // Lifting both, however quick, is no tap.
        assert!(t.up(2, 50.0).is_empty());
        assert!(t.up(1, 60.0).is_empty());
        // Nor is one finger that joined a second.
        t.down(1, (0.0, 0.0), 0.0);
        t.down(2, (10.0, 0.0), 5.0);
        assert!(t.up(2, 10.0).is_empty());
        assert!(t.up(1, 20.0).is_empty());
    }

    #[test]
    fn a_long_press_fires_once_and_then_the_lift_is_not_a_tap() {
        let mut t = Touches::default();
        t.down(1, (10.0, 20.0), 1000.0);
        assert!(t.poll(1300.0).is_empty());
        assert_eq!(t.poll(1500.0), [TouchAction::LongPress((10.0, 20.0))]);
        assert!(t.poll(1900.0).is_empty(), "once");
        assert!(t.up(1, 2000.0).is_empty());
        // Moving cancels the press.
        t.down(1, (10.0, 20.0), 0.0);
        t.moved(1, (60.0, 20.0));
        assert!(t.poll(900.0).is_empty());
        t.cancel();
        assert!(!t.active());
    }
}
