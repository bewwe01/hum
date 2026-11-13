//! Multi-touch input. egui's built-in widgets follow a single pointer, but
//! Hum needs several fingers at once (chords, a finger on the pad while
//! another plays), so every custom widget reads touches from here.
//!
//! On desktop the mouse acts as one finger.

use eframe::egui::{self, Event, Id, PointerButton, Pos2, Rect, TouchPhase};

/// Finger movement (in points) beyond which a touch is a drag, not a tap.
pub const TAP_SLOP: f32 = 12.0;
const MOUSE_ID: u64 = u64::MAX;

#[derive(Clone, Copy, Debug)]
pub struct Touch {
    pub id: u64,
    pub pos: Pos2,
    pub start: Pos2,
    /// Position at the previous frame.
    pub prev: Pos2,
    /// Began this frame.
    pub started: bool,
    /// Lifted (or cancelled) this frame; gone next frame.
    pub ended: bool,
    pub cancelled: bool,
    /// Largest distance from the start position so far.
    pub travel: f32,
    pub owner: Option<Id>,
}

impl Touch {
    pub fn delta(&self) -> egui::Vec2 {
        self.pos - self.prev
    }

    pub fn is_tap(&self) -> bool {
        self.ended && !self.cancelled && self.travel < TAP_SLOP
    }
}

/// Result of [`Touches::button`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Tap {
    /// A finger is currently down on the button.
    pub down: bool,
    /// The button was tapped this frame.
    pub clicked: bool,
    /// A finger went down on the button this frame.
    pub pressed: bool,
}

#[derive(Default)]
pub struct Touches {
    list: Vec<Touch>,
    last_touch_event: f64,
    blocked: bool,
}

impl Touches {
    /// Collect this frame's input. Call once at the start of each frame.
    pub fn begin_frame(&mut self, ctx: &egui::Context) {
        self.list.retain(|t| !t.ended);
        for t in &mut self.list {
            t.started = false;
            t.prev = t.pos;
        }
        self.blocked = false;
        let (events, now) = ctx.input(|i| (i.events.clone(), i.time));
        for event in &events {
            match *event {
                Event::Touch { id, phase, pos, .. } => {
                    self.last_touch_event = now;
                    // A real finger replaces the emulated mouse pointer.
                    self.list.retain(|t| t.id != MOUSE_ID);
                    match phase {
                        TouchPhase::Start => self.start(id.0, pos),
                        TouchPhase::Move => self.moved(id.0, pos),
                        TouchPhase::End => self.end(id.0, Some(pos), false),
                        TouchPhase::Cancel => self.end(id.0, None, true),
                    }
                }
                // egui also reports touches as an emulated mouse; ignore
                // that while real touch events are arriving.
                _ if now - self.last_touch_event < 1.0 && self.last_touch_event > 0.0 => {}
                Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed,
                    ..
                } => {
                    if pressed {
                        self.start(MOUSE_ID, pos);
                    } else {
                        self.end(MOUSE_ID, Some(pos), false);
                    }
                }
                Event::PointerMoved(pos) => self.moved(MOUSE_ID, pos),
                Event::PointerGone | Event::WindowFocused(false) => self.end(MOUSE_ID, None, true),
                _ => {}
            }
        }
    }

    fn start(&mut self, id: u64, pos: Pos2) {
        self.list.retain(|t| t.id != id);
        self.list.push(Touch {
            id,
            pos,
            start: pos,
            prev: pos,
            started: true,
            ended: false,
            cancelled: false,
            travel: 0.0,
            owner: None,
        });
    }

    fn moved(&mut self, id: u64, pos: Pos2) {
        if let Some(t) = self.list.iter_mut().find(|t| t.id == id && !t.ended) {
            t.pos = pos;
            t.travel = t.travel.max((pos - t.start).length());
        }
    }

    fn end(&mut self, id: u64, pos: Option<Pos2>, cancelled: bool) {
        if let Some(t) = self.list.iter_mut().find(|t| t.id == id && !t.ended) {
            if let Some(pos) = pos {
                t.pos = pos;
                t.travel = t.travel.max((pos - t.start).length());
            }
            t.ended = true;
            t.cancelled = cancelled;
        }
    }

    /// True while any finger is down (used to keep repainting).
    pub fn any_active(&self) -> bool {
        !self.list.is_empty()
    }

    /// While blocked, widgets cannot claim new touches (used for the screen
    /// underneath a modal sheet).
    pub fn set_blocked(&mut self, blocked: bool) {
        self.blocked = blocked;
    }

    /// Cancel every touch (e.g. when the app loses focus).
    pub fn cancel_all(&mut self) {
        for t in &mut self.list {
            t.ended = true;
            t.cancelled = true;
        }
    }

    /// Give `owner` every new, unclaimed touch that started inside `rect`.
    pub fn claim(&mut self, owner: Id, rect: Rect) {
        if self.blocked {
            return;
        }
        for t in &mut self.list {
            if t.started && t.owner.is_none() && rect.contains(t.start) {
                t.owner = Some(owner);
            }
        }
    }

    /// The touches owned by `owner` (including ones that ended this frame).
    pub fn owned(&self, owner: Id) -> impl Iterator<Item = &Touch> + '_ {
        self.list.iter().filter(move |t| t.owner == Some(owner))
    }

    /// Treat `rect` as a push button.
    pub fn button(&mut self, owner: Id, rect: Rect) -> Tap {
        self.claim(owner, rect);
        let mut tap = Tap::default();
        let slack = rect.expand(10.0);
        for t in self.owned(owner) {
            if t.started {
                tap.pressed = true;
            }
            if t.ended {
                if t.is_tap() && slack.contains(t.pos) {
                    tap.clicked = true;
                }
            } else if t.travel < TAP_SLOP && slack.contains(t.pos) {
                tap.down = true;
            }
        }
        tap
    }
}
