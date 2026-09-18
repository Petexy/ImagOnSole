//! What the viewer is looking at, and what every control does to it.
//!
//! Nothing here draws. `draw.rs` reads this and puts it on the screen, and the
//! window in `main.rs` hands it actions — so what a button does is decided in
//! one place, whether the button was on a pad, a keyboard or a mouse.
//!
//! **The one shape everything else follows.** A photograph is drawn over the
//! frame the toolkit composed, so nothing the toolkit draws can appear on top
//! of one (see `photo.rs`). Rather than fight that, the picture is given a
//! *stage* — a rectangle it may occupy — and everything else is laid out
//! outside it. When a menu opens the stage narrows to make room; when a dialog
//! or the file chooser takes over, the picture fades out entirely. That is why
//! `stage` is animated state rather than a rectangle worked out while drawing.

use std::path::{Path, PathBuf};

use lxb_render::{ContextMenu, Dialog, Entry as MenuEntry, Files, Selection as Light, Spot};
use lxb_toolkit::{
    input::Action,
    menu,
    metrics::Metric,
    motion::{self, spring},
    sound::Sound,
    typography::Text,
};

use crate::library::{Folder, Order};

/// Pointer targets. Kept clear of the legend's, which count from `0x8000`.
pub const CARD: u32 = 0x100;
pub const STAGE: u32 = 0x20;
pub const SCROLL_BAR: u32 = 0x21;

/// How long one photograph is held before a slideshow moves on.
const SLIDE: f32 = 5.0;

/// How long one photograph takes to cross to the next.
///
/// **Half again as fast as a panel's own transition**, and derived from it so
/// that it stays tied to the rhythm of everything else rather than being a
/// number somebody chose. Stepping through a folder is the one thing anybody
/// does *repeatedly* in this application — press, look, press, look — and at a
/// panel's speed it read as the viewer thinking about it between pictures.
/// Asked for by the user on 2026-08-31, who called it sluggish.
const CROSSFADE: f32 = motion::duration::PANEL / 1.5;

/// The zoom stops a press walks round.
///
/// Fit, then the picture at its own size, then twice and four times that. A
/// ladder rather than a continuous zoom because one button has to reach every
/// useful size, and because a stop somebody can name — *actual size* — is
/// worth more than a number they have to steer to.
const STOPS: [Zoom; 4] = [
    Zoom::Fit,
    Zoom::Actual(1.0),
    Zoom::Actual(2.0),
    Zoom::Actual(4.0),
];

/// How much one notch of a wheel changes the zoom.
///
/// Small enough that a slow turn is a smooth zoom rather than a series of
/// jumps, and large enough that a few notches get somewhere.
const NOTCH: f32 = 1.16;

/// How much a trigger held all the way in changes the zoom in a second.
const PULL_RATE: f32 = 3.0;

/// How far in a picture may be taken. Past thirty-two pixels to the pixel
/// there is nothing left in the file to look at.
///
/// There is no matching number for the other end: **a picture is never taken
/// out past fitting**. Smaller than that is a photograph adrift in a screen of
/// wallpaper, which is a state to have to get back out of rather than a way of
/// looking at anything.
const MOST: f32 = 32.0;

/// How near fitting counts as fitting.
///
/// Deliberately far smaller than any single thing that changes the zoom — a
/// wheel notch is sixteen percent and a frame of held trigger about two —
/// because this snaps the zoom back to `Fit`, and a window wider than one
/// step of a *continuous* control swallows every step: the picture would be
/// put back where it started on every frame and a held trigger would do
/// nothing at all. That is exactly what it did.
const AS_GOOD_AS_FIT: f32 = 0.005;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Zoom {
    Fit,
    /// Drawn pixels per pixel of the file.
    Actual(f32),
}

/// Which way a change of page is going, if one is.
///
/// A photograph opens **out of the card it was pressed on** and shrinks back
/// into it, and one number drives the whole of both: where the picture is, how
/// round its corners are, and how far the wall of pictures behind it has
/// stepped back. Two clocks would land the picture and the page around it on
/// different frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Going {
    Nowhere,
    /// Out of the card and on to the screen.
    In,
    /// Back into the card it came out of.
    Out,
}

/// How much of the way back into its card a picture dissolves over.
///
/// It has to be there for most of the way — a picture that faded at the press
/// would land nothing on the card — and gone by the time it arrives, so what
/// the card is left holding is its own thumbnail rather than a cut from the
/// full-sized picture at a slightly different size.
const LANDS_OVER: f32 = 0.35;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Grid,
    Viewer,
}

/// What a row of the Options menu does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    Turn(i32),
    ZoomTo(Zoom),
    Info,
    Slideshow,
    OpenFolder,
    UpAFolder,
    ShowHidden,
    SortBy(Order),
    BackToGrid,
}

/// How fast a held direction pans, and how quickly it gets there.
///
/// The shell moves a floating window on a ramp like this rather than at one
/// speed, for the same reason: a rate that crosses a large picture in a
/// reasonable time is far too fast to land on anything, and one that lands
/// well never gets across.
const PAN_FROM: f32 = 340.0;
const PAN_TO: f32 = 2800.0;
const PAN_RAMP: f32 = 2.4;
/// How long a push is believed after the last one arrived.
///
/// Longer than the repeat delay before the second action of a hold, or a pan
/// would stop dead a third of a second in and start again.
const PAN_PATIENCE: f32 = 0.42;

/// A menu that has been asked for and not yet raised.
///
/// Every row here is a fixed phrase and fits by construction. The title is
/// the picture's own name and does not, which is what this waits for
/// something that can measure a word to settle. See [`View::settle_menu`].
struct Asked {
    title: String,
    entries: Vec<MenuEntry>,
    anchor: [f32; 4],
    window: usize,
}

pub struct View {
    pub folder: Folder,
    pub order: Order,
    pub hidden: bool,
    /// Where in `folder.entries` the light is, in the grid and in the viewer
    /// alike — the two are one selection, which is what makes leaving the
    /// viewer put the grid back where it was.
    pub cursor: usize,
    pub mode: Mode,

    /// Worked out while drawing, kept because acting on a direction needs it.
    pub columns: usize,
    pub visible_rows: f32,
    pub scroll: f32,
    pub scroll_speed: f32,

    pub light: Light,
    pub pressing: lxb_render::Pressing,

    pub zoom: Zoom,
    /// The drawn scale, which chases whatever `zoom` asks for.
    pub scale: f32,
    pub scale_speed: f32,
    /// Measured once a frame, before anything acts on it: the scale at which
    /// the picture fits the stage, the size it is actually being drawn, and
    /// how far it hangs over each edge. Acting on a direction has to know all
    /// three *before* the frame is drawn, which is why they are kept rather
    /// than worked out where they are used.
    pub fit: f32,
    /// The room the picture takes on the screen, turn included.
    pub drawn: [f32; 2],
    /// The picture's own drawn size, before the turn — which is the quad.
    pub quad: [f32; 2],
    pub overflow: [f32; 2],
    wanted_scale: f32,
    pub pan: [f32; 2],
    pub pan_speed: [f32; 2],
    push: [f32; 2],
    pushed_for: [f32; 2],
    since_push: [f32; 2],

    pub quarters: i32,
    pub turn: f32,
    pub turn_speed: f32,
    /// How hard the triggers are pulled, right less left.
    pull: f32,

    pub info: bool,
    pub info_out: f32,
    /// How much of the window a slideshow has taken.
    ///
    /// Its own eased number because `slideshow` is a flag, and the stage is
    /// set from its target rather than sprung at it — an abrupt input to a
    /// sum that nothing smooths any more is a picture that jumps.
    slideshow_out: f32,

    pub slideshow: bool,
    pub slide_left: f32,

    pub menu: ContextMenu,
    pub commands: Vec<Command>,
    /// A menu that has been asked for and not yet raised.
    ///
    /// It waits one step, until `draw.rs` has something that can measure a
    /// word. The picture's name is the title of its menu, and a title has to
    /// be cut to the panel before it is handed over — see
    /// [`View::settle_menu`].
    asked: Option<Asked>,
    pub dialog: Dialog,
    pub files: Files,

    going: Going,
    /// How much of a change of page is still to come: 1 at the press, 0 when
    /// the picture lands. See [`Going`] and [`View::grown`].
    to_go: f32,
    /// The far end of a change of page: where the picture is going, or where
    /// it is coming back from.
    ///
    /// Followed rather than read, on the way in, so that a stage target which
    /// moves under the animation — the details opening, a menu asking for its
    /// room — cannot jump it. Frozen on the way out, where the picture is
    /// coming back from a place that has stopped existing.
    crossing_far: [f32; 4],

    /// The rectangle the photograph may occupy.
    ///
    /// **Set rather than sprung.** Everything it is worked out from is already
    /// an eased number, so a spring on top of them bought nothing but lag —
    /// and the lag was the bug: the details pane arrives at the speed of its
    /// own fade and the stage crawled after it, which left the photograph
    /// drawn over a pane it is supposed to stand beside. The one thing that
    /// moves it abruptly is a change of page, and that sets it too.
    pub stage: [f32; 4],
    pub stage_known: bool,
    /// How round the picture on a card is, as of the last measurement. Kept
    /// for the same reason `window` is: what the picture is drawn with has to
    /// be answered where there is no `Geometry` to hand.
    frame_radius: f32,
    /// The photograph's own size in pixels, as of the last measurement. Kept
    /// so that how large it is drawn can be worked out again once the stage
    /// has moved, without asking the reader a second time.
    picture: [f32; 2],
    /// The window, as of the last measurement. Kept because a menu is opened
    /// by an action rather than while drawing, and it has to know where the
    /// stage is about to stand aside to before the stage has moved.
    window: [f32; 2],

    /// How much of the photograph is on the screen, and the one going away.
    pub showing: f32,
    pub leaving: Option<(PathBuf, crate::photo::Placement)>,
    pub leaving_out: f32,

    pub note: Option<String>,
    pub quit: bool,
    /// This was started on one picture rather than on a folder — a file
    /// manager handing over a double-click, rather than somebody opening the
    /// viewer to browse. Back closes the application while it holds. See
    /// [`View::closes_on_back`].
    opened_on_a_picture: bool,
    /// The folder this walk is rooted at, which is the one it was opened on.
    ///
    /// Back walks *up* to it and closes *at* it, rather than carrying on
    /// through the home directory and out to the root of the disk: somebody
    /// who opened a folder asked for that folder, and the four presses it used
    /// to take to leave went through three they had never asked to see. Going
    /// above it is still one row of the Options menu away, and doing that
    /// moves the root with them.
    top: PathBuf,
}

impl View {
    pub fn new(at: &Path, order: Order, hidden: bool) -> View {
        let (folder, cursor, mode) = open_at(at, order, hidden);
        let folder_path = folder.path.clone();
        View {
            folder,
            order,
            hidden,
            cursor,
            mode,
            columns: 4,
            visible_rows: 3.0,
            scroll: 0.0,
            scroll_speed: 0.0,
            light: Light::default(),
            pressing: lxb_render::Pressing::default(),
            zoom: Zoom::Fit,
            scale: 0.0,
            scale_speed: 0.0,
            fit: 0.0,
            drawn: [0.0; 2],
            quad: [0.0; 2],
            overflow: [0.0; 2],
            wanted_scale: 0.0,
            pan: [0.0; 2],
            pan_speed: [0.0; 2],
            push: [0.0; 2],
            pushed_for: [0.0; 2],
            since_push: [f32::MAX; 2],
            quarters: 0,
            turn: 0.0,
            turn_speed: 0.0,
            pull: 0.0,
            info: false,
            info_out: 0.0,
            slideshow_out: 0.0,
            slideshow: false,
            slide_left: SLIDE,
            menu: ContextMenu::default(),
            commands: Vec::new(),
            asked: None,
            dialog: Dialog::default(),
            files: Files::default(),
            going: Going::Nowhere,
            to_go: 0.0,
            crossing_far: [0.0; 4],
            stage: [0.0; 4],
            stage_known: false,
            frame_radius: 0.0,
            picture: [0.0; 2],
            // Until the first measurement. A menu raised before a single
            // frame has been drawn would otherwise be anchored at the origin;
            // this is only ever wrong for that one frame.
            window: [1280.0, 800.0],
            showing: 0.0,
            leaving: None,
            leaving_out: 0.0,
            note: None,
            quit: false,
            opened_on_a_picture: mode == Mode::Viewer,
            top: folder_path,
        }
    }

    pub fn current(&self) -> Option<&crate::library::Entry> {
        self.folder.entries.get(self.cursor)
    }

    /// The photograph being looked at, which is only a photograph in the
    /// viewer: in the grid the light may well be on a folder.
    pub fn photograph(&self) -> Option<&Path> {
        let entry = self.current()?;
        (!entry.is_folder()).then_some(entry.path.as_path())
    }

    /// The pictures worth having decoded: the one on screen, then the ones
    /// either side of it, so that stepping shows a photograph rather than a
    /// wait for one.
    pub fn wanted(&self) -> Vec<PathBuf> {
        let mut wanted = Vec::new();
        if self.mode == Mode::Viewer {
            if let Some(path) = self.photograph() {
                wanted.push(path.to_path_buf());
            }
            for step in [1isize, -1] {
                if let Some(next) = self.folder.step(self.cursor, step) {
                    let path = &self.folder.entries[next].path;
                    if !wanted.contains(path) {
                        wanted.push(path.clone());
                    }
                }
            }
        }
        if let Some((path, _)) = &self.leaving {
            if !wanted.contains(path) {
                wanted.push(path.clone());
            }
        }
        wanted
    }

    /// Is something the toolkit drew standing over the page?
    ///
    /// The photograph has to give way to all three, in the two different ways
    /// described at the top of this file.
    pub fn overlaid(&self) -> bool {
        self.menu.is_open() || self.dialog.is_open() || self.files.busy()
    }

    /// The overlays that take the whole screen rather than a corner of it.
    fn takes_over(&self) -> bool {
        self.dialog.is_open() || self.files.busy()
    }

    // ---- what the controls do -------------------------------------------

    pub fn act(&mut self, action: Action) -> Option<Sound> {
        // Every panel the toolkit owns reads its own actions first, and closes
        // one thing at a time, exactly as it does under `lxb-app`.
        if self.files.busy() {
            return self.files.act(action);
        }
        if self.dialog.is_open() {
            return self.dialog_act(action);
        }
        if self.menu.is_open() {
            return self.menu_act(action);
        }
        match self.mode {
            Mode::Grid => self.grid_act(action),
            Mode::Viewer => self.viewer_act(action),
        }
    }

    fn dialog_act(&mut self, action: Action) -> Option<Sound> {
        match action {
            Action::Left => {
                self.dialog.step(-1);
                Some(Sound::Move)
            }
            Action::Right => {
                self.dialog.step(1);
                Some(Sound::Move)
            }
            Action::Accept => {
                self.dialog.press();
                self.dialog.close();
                Some(Sound::Press)
            }
            Action::Back => {
                self.dialog.close();
                Some(Sound::Back)
            }
            _ => None,
        }
    }

    fn menu_act(&mut self, action: Action) -> Option<Sound> {
        match action {
            Action::Up => {
                self.menu.step(-1);
                Some(Sound::Move)
            }
            Action::Down => {
                self.menu.step(1);
                Some(Sound::Move)
            }
            Action::Accept => {
                self.menu.press();
                let chosen = self.commands.get(self.menu.selected()).copied();
                self.menu.close();
                if let Some(command) = chosen {
                    return self.run(command).or(Some(Sound::Press));
                }
                Some(Sound::Press)
            }
            Action::Back | Action::Menu => {
                self.menu.close();
                Some(Sound::Back)
            }
            _ => None,
        }
    }

    fn grid_act(&mut self, action: Action) -> Option<Sound> {
        let count = self.folder.entries.len();
        match action {
            Action::Left | Action::Right | Action::Up | Action::Down => {
                if count == 0 {
                    return None;
                }
                let columns = self.columns.max(1) as isize;
                let step = match action {
                    Action::Left => -1,
                    Action::Right => 1,
                    Action::Up => -columns,
                    _ => columns,
                };
                let at = self.cursor as isize + step;
                // A grid stops at its edges rather than wrapping: wrapping a
                // row takes the light to the far side of the screen, which is
                // exactly where somebody pressing Right was not looking.
                if at < 0 || at >= count as isize {
                    return None;
                }
                self.cursor = at as usize;
                Some(Sound::Move)
            }
            Action::Previous | Action::Next => {
                if count == 0 {
                    return None;
                }
                let page = (self.columns.max(1) as f32 * self.visible_rows).max(1.0) as isize;
                let step = if action == Action::Next { page } else { -page };
                let at = (self.cursor as isize + step).clamp(0, count as isize - 1);
                if at as usize == self.cursor {
                    return None;
                }
                self.cursor = at as usize;
                Some(Sound::Move)
            }
            Action::Accept => self.enter(),
            Action::Back => self.leave(),
            Action::Menu => {
                self.open_menu();
                Some(Sound::Press)
            }
            Action::Submit => {
                // Start a slideshow from wherever the light is, which for a
                // folder means the first photograph in it.
                if self.folder.photographs() == 0 {
                    return Some(Sound::Error);
                }
                if self.current().is_some_and(|entry| entry.is_folder()) {
                    let first = self
                        .folder
                        .entries
                        .iter()
                        .position(|entry| !entry.is_folder());
                    let Some(first) = first else {
                        return Some(Sound::Error);
                    };
                    self.cursor = first;
                }
                self.show_the_picture();
                self.slideshow = true;
                self.slide_left = SLIDE;
                Some(Sound::Press)
            }
        }
    }

    fn viewer_act(&mut self, action: Action) -> Option<Sound> {
        match action {
            Action::Left | Action::Right | Action::Up | Action::Down => {
                let (axis, sign) = match action {
                    Action::Left => (0, -1.0),
                    Action::Right => (0, 1.0),
                    Action::Up => (1, -1.0),
                    _ => (1, 1.0),
                };
                // The rule the viewer is built on: a direction pans where
                // there is something to pan to, and otherwise it is a step to
                // the next picture. Nothing has to be switched on, and the
                // picture itself says which it is going to be.
                if self.overflows(axis) {
                    self.push[axis] = sign;
                    self.since_push[axis] = 0.0;
                    None
                } else if axis == 0 {
                    self.step(sign as isize)
                } else {
                    None
                }
            }
            Action::Previous => self.step(-1),
            Action::Next => self.step(1),
            Action::Accept => {
                self.next_stop();
                Some(Sound::Press)
            }
            Action::Back => self.leave(),
            Action::Menu => {
                self.open_menu();
                Some(Sound::Press)
            }
            Action::Submit => {
                self.slideshow = !self.slideshow;
                self.slide_left = SLIDE;
                Some(Sound::Press)
            }
        }
    }

    /// A press on the stage, from a pointer.
    pub fn press_stage(&mut self) -> Option<Sound> {
        self.next_stop();
        Some(Sound::Press)
    }

    /// What one zoom really works out to, in drawn pixels per pixel of the
    /// file. `Fit` is not a number until there is a picture to fit.
    pub fn scale_of(&self, zoom: Zoom) -> f32 {
        match zoom {
            Zoom::Fit => self.fit,
            Zoom::Actual(scale) => scale,
        }
    }

    pub fn effective_scale(&self) -> f32 {
        self.scale_of(self.zoom)
    }

    /// The stops a press walks, in the order they really come out in.
    ///
    /// Sorted rather than written down in order, because *fit* is a number
    /// that depends on the picture: a photograph smaller than the stage fits
    /// at more than one pixel to the pixel, so on that picture *fit* is not
    /// the first stop. Two stops within a couple of percent of each other are
    /// one stop — a press that appeared to do nothing would read as the
    /// viewer's fault rather than the picture's.
    fn ladder(&self) -> Vec<Zoom> {
        let mut stops: Vec<Zoom> = STOPS
            .into_iter()
            .filter(|stop| self.scale_of(*stop) > 0.0)
            .collect();
        stops.sort_by(|left, right| {
            self.scale_of(*left)
                .partial_cmp(&self.scale_of(*right))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        stops.dedup_by(|left, right| {
            let (left, right) = (self.scale_of(*left), self.scale_of(*right));
            right > 0.0 && (left - right).abs() / right < 0.02
        });
        stops
    }

    /// The next stop above where the picture is now, coming round to the
    /// smallest at the top.
    ///
    /// Asked against the scale rather than against which stop was last chosen,
    /// so a press after a wheel or a trigger goes on from where the picture
    /// actually is instead of from wherever the ladder was left.
    fn next_stop(&mut self) {
        let stops = self.ladder();
        if stops.is_empty() {
            return;
        }
        let now = self.effective_scale();
        let next = stops
            .iter()
            .find(|stop| self.scale_of(**stop) > now * 1.02)
            .copied()
            .unwrap_or(stops[0]);
        self.settle_on(next);
    }

    fn settle_on(&mut self, zoom: Zoom) {
        self.zoom = zoom;
        if zoom == Zoom::Fit {
            self.pan = [0.0; 2];
        }
    }

    /// Zoom by a factor, keeping one point on the screen where it is.
    ///
    /// This is the continuous way in — a wheel, or a trigger — and it is
    /// deliberately *not* sprung: the smoothness comes from the control
    /// itself, and a spring between the hand and the picture would only feel
    /// like lag. The stops still spring, because a stop is a jump.
    ///
    /// `about` is where to keep still, in the window's own pixels. A wheel
    /// gives the pointer, so the picture grows under the hand rather than away
    /// from it; a trigger gives nothing and the middle of the stage is used,
    /// because a controller has no pointer to grow towards.
    pub fn zoom_by(&mut self, factor: f32, about: Option<[f32; 2]>) {
        if self.mode != Mode::Viewer || self.fit <= 0.0 || !factor.is_finite() {
            return;
        }
        let from = self.effective_scale();
        if from <= 0.0 {
            return;
        }
        // Never smaller than fitting, and never past what is in the file.
        // Taken in that order, because on a picture far smaller than the
        // screen the size it fits at is itself past `MOST`, and a ceiling
        // below the floor would pin every zoom to one number.
        let least = self.fit;
        let to = (from * factor).clamp(least, MOST.max(least));
        let moved = to / from;
        if (moved - 1.0).abs() < 0.0001 {
            return;
        }

        // Keep the named point still. The picture scales about its own centre,
        // so where that centre has to move follows from where the point is
        // relative to it — and none of it depends on the turn, because a turn
        // is about that same centre.
        let middle = [
            self.stage[0] + self.stage[2] * 0.5,
            self.stage[1] + self.stage[3] * 0.5,
        ];
        let about = about.unwrap_or(middle);
        for axis in 0..2 {
            let from_middle = about[axis] - middle[axis];
            self.pan[axis] = from_middle - (from_middle - self.pan[axis]) * moved;
        }

        // Within a hair of fitting *is* fitting, so the word on the page, the
        // ladder and the picture all agree.
        if (to - self.fit).abs() / self.fit < AS_GOOD_AS_FIT {
            self.zoom = Zoom::Fit;
            self.pan = [0.0; 2];
        } else {
            self.zoom = Zoom::Actual(to);
        }
        // Straight there rather than sprung, and the measurements with it, so
        // that the point under the hand is still on this frame rather than two
        // frames from now.
        self.wanted_scale = self.scale_of(self.zoom);
        self.scale = self.wanted_scale;
        self.scale_speed = 0.0;
        if self.quad[0] > 0.0 {
            let grew = self.scale / from;
            self.quad = [self.quad[0] * grew, self.quad[1] * grew];
            self.drawn = [self.drawn[0] * grew, self.drawn[1] * grew];
            self.overflow = [
                (self.drawn[0] - self.stage[2]).max(0.0),
                (self.drawn[1] - self.stage[3]).max(0.0),
            ];
        }
        self.clamp_pan();
    }

    /// How hard the triggers are pulled, from the pad. Applied every frame in
    /// [`View::advance`] rather than acted on once, because a trigger is held
    /// rather than pressed.
    pub fn set_pull(&mut self, pull: f32) {
        self.pull = if pull.is_finite() {
            pull.clamp(-1.0, 1.0)
        } else {
            0.0
        };
    }

    /// A wheel, and where it was pointed.
    ///
    /// **In the viewer a wheel zooms.** Everywhere else it is what it is
    /// everywhere else in this language: Up and Down, so a grid scrolls and an
    /// open menu walks its rows without either having heard of a wheel.
    /// `notches` is positive downwards, as the toolkit reports it.
    pub fn wheel(&mut self, notches: i32, at: [f32; 2]) -> Option<Sound> {
        if notches == 0 {
            return None;
        }
        if self.mode == Mode::Viewer && !self.overlaid() {
            // Down is away, which is what every other picture on this desktop
            // does with a wheel.
            self.zoom_by(NOTCH.powi(-notches), Some(at));
            // Silent: a wheel zoom lands on nothing, and the language keeps a
            // move that landed nowhere quiet.
            return None;
        }
        let action = if notches > 0 {
            Action::Down
        } else {
            Action::Up
        };
        let mut answer = None;
        for _ in 0..notches.abs() {
            answer = self.act(action).or(answer);
        }
        answer
    }

    fn step(&mut self, step: isize) -> Option<Sound> {
        let Some(next) = self.folder.step(self.cursor, step) else {
            return Some(Sound::Error);
        };
        if let Some(path) = self.photograph() {
            self.leaving = Some((path.to_path_buf(), self.placement_now()));
            self.leaving_out = 1.0;
        }
        self.cursor = next;
        self.reset_picture();
        self.showing = 0.0;
        self.slide_left = SLIDE;
        Some(Sound::Move)
    }

    fn enter(&mut self) -> Option<Sound> {
        let Some(entry) = self.current() else {
            return Some(Sound::Error);
        };
        if entry.is_folder() {
            let into = entry.path.clone();
            self.go_to(&into, None);
            return Some(Sound::Press);
        }
        self.show_the_picture();
        Some(Sound::Press)
    }

    fn show_the_picture(&mut self) {
        self.mode = Mode::Viewer;
        self.reset_picture();
        self.showing = 0.0;
        self.leaving = None;
        // The stage is not told where it is going yet — `draw` works that out
        // from the window — but it is told to start from the card, so that
        // opening a photograph grows out of the one that was pressed.
        self.stage_known = false;
        self.going = Going::In;
        self.to_go = 1.0;
        // Where it is going is not known yet — see [`View::crossing_far`].
        self.crossing_far = [0.0; 4];
    }

    /// Start the picture on its way back into its card.
    ///
    /// It goes on being drawn, shrinking, until [`View::advance`] sees the
    /// crossing land — a picture let go at the press would vanish halfway back
    /// to the card it came out of.
    fn part_with_the_picture(&mut self) {
        self.going = Going::Out;
        self.to_go = 1.0;
        self.crossing_far = self.stage;
        // A picture goes back into its card *whole*. One left zoomed in would
        // shrink to a card holding a corner of itself, and one left panned
        // would land beside the card rather than on it — so the zoom unwinds
        // through `fit_the_picture` and the pan through `placement_now`.
        self.zoom = Zoom::Fit;
    }

    /// Whether Back closes the application rather than going anywhere.
    ///
    /// True while this is showing the one picture it was started on. Somebody
    /// who double-clicked a photograph in a file manager asked to see *that
    /// photograph*; the folder behind it is there so the arrows have somewhere
    /// to go, not because they asked to browse it, and dropping them into a
    /// grid they never opened is a worse answer than closing.
    ///
    /// It stops being true the moment they say otherwise — **Back to the
    /// folder**, on the Options menu, is how they do that, and from then on
    /// this is an ordinary browse and Back walks back out of it.
    pub fn closes_on_back(&self) -> bool {
        match self.mode {
            Mode::Viewer => self.opened_on_a_picture,
            Mode::Grid => self.at_the_top(),
        }
    }

    /// Whether this folder is as far out as the walk goes.
    ///
    /// The folder it was opened on, or one with nothing above it at all —
    /// which is the same answer for the same reason.
    fn at_the_top(&self) -> bool {
        self.folder.path == self.top
            || self
                .folder
                .path
                .parent()
                .is_none_or(|up| up == self.folder.path)
    }

    fn leave(&mut self) -> Option<Sound> {
        match self.mode {
            Mode::Viewer if self.opened_on_a_picture => {
                self.quit = true;
                Some(Sound::Back)
            }
            Mode::Viewer => {
                self.mode = Mode::Grid;
                self.slideshow = false;
                self.leaving = None;
                self.part_with_the_picture();
                Some(Sound::Back)
            }
            // Nowhere further out to go: leaving the top is leaving.
            Mode::Grid if self.at_the_top() => {
                self.quit = true;
                Some(Sound::Back)
            }
            Mode::Grid => {
                let here = self.folder.path.clone();
                match here.parent() {
                    Some(up) if up != here => {
                        let up = up.to_path_buf();
                        self.go_to(&up, Some(&here));
                        Some(Sound::Back)
                    }
                    _ => {
                        self.quit = true;
                        Some(Sound::Back)
                    }
                }
            }
        }
    }

    /// Open a folder chosen outright — from the chooser — which roots the walk
    /// there. Walking *into* a folder does not: that is a step inside the walk
    /// rather than the start of a new one.
    pub fn browse_from(&mut self, path: &Path) {
        self.top = path.to_path_buf();
        self.go_to(path, None);
    }

    pub fn go_to(&mut self, path: &Path, land_on: Option<&Path>) {
        self.opened_on_a_picture = false;
        self.folder = Folder::read(path, self.order, self.hidden);
        self.cursor = land_on
            .and_then(|was| self.folder.position(was))
            .unwrap_or(0);
        self.mode = Mode::Grid;
        self.slideshow = false;
        self.scroll = 0.0;
        self.scroll_speed = 0.0;
        self.light.clear();
        self.leaving = None;
        self.note = self
            .folder
            .unreadable
            .then(|| String::from(crate::i18n::text("this-folder-cannot-be-opened")));
    }

    fn reread(&mut self) {
        let here = self.folder.path.clone();
        let was = self.current().map(|entry| entry.path.clone());
        self.folder = Folder::read(&here, self.order, self.hidden);
        self.cursor = was
            .and_then(|was| self.folder.position(&was))
            .unwrap_or(0)
            .min(self.folder.entries.len().saturating_sub(1));
        // A picture that has just been sorted out of the listing cannot go on
        // being looked at.
        if self.photograph().is_none() {
            self.mode = Mode::Grid;
            self.slideshow = false;
        }
    }

    fn reset_picture(&mut self) {
        self.zoom = Zoom::Fit;
        self.pan = [0.0; 2];
        self.pan_speed = [0.0; 2];
        self.push = [0.0; 2];
        self.quarters = 0;
        self.scale = 0.0;
        self.scale_speed = 0.0;
        self.info_out = self.info_out.min(if self.info { 1.0 } else { 0.0 });
    }

    fn run(&mut self, command: Command) -> Option<Sound> {
        match command {
            Command::Turn(quarters) => {
                self.quarters += quarters;
                // A turn changes which way the picture is longest, so
                // whatever it was zoomed to no longer means the same thing.
                self.zoom = Zoom::Fit;
                self.pan = [0.0; 2];
                None
            }
            Command::ZoomTo(zoom) => {
                self.zoom = zoom;
                if zoom == Zoom::Fit {
                    self.pan = [0.0; 2];
                }
                None
            }
            Command::Info => {
                self.info = !self.info;
                None
            }
            Command::Slideshow => {
                self.slideshow = !self.slideshow;
                self.slide_left = SLIDE;
                None
            }
            Command::OpenFolder => {
                self.files.a_folder(&self.folder.path);
                None
            }
            Command::UpAFolder => {
                let here = self.folder.path.clone();
                if let Some(up) = here.parent().filter(|up| **up != here) {
                    let up = up.to_path_buf();
                    // Deliberately stepping over the top of the walk moves the
                    // top with them. There is no sense in a root somebody has
                    // just asked to go above — Back would close from a folder
                    // further out than the one it closes at.
                    if here == self.top {
                        self.top = up.clone();
                    }
                    self.go_to(&up, Some(&here));
                    return Some(Sound::Back);
                }
                Some(Sound::Error)
            }
            Command::ShowHidden => {
                self.hidden = !self.hidden;
                self.reread();
                None
            }
            Command::SortBy(order) => {
                self.order = order;
                self.reread();
                None
            }
            Command::BackToGrid => {
                // Asking for the folder is asking to browse it, so from here
                // on this is an ordinary walk and Back walks back out of it
                // rather than closing. See `closes_on_back`.
                self.opened_on_a_picture = false;
                self.mode = Mode::Grid;
                self.slideshow = false;
                self.part_with_the_picture();
                Some(Sound::Back)
            }
        }
    }

    // ---- the menu --------------------------------------------------------

    /// Ask for the Options menu.
    ///
    /// It is *asked for* rather than opened, and raised by
    /// [`View::settle_menu`] on the same frame. Nothing here can measure a
    /// word, and the title of a picture's menu is the picture's own name.
    pub fn open_menu(&mut self) {
        let (title, rows) = self.menu_rows();
        self.commands = rows.iter().map(|(_, command)| *command).collect();
        let entries: Vec<MenuEntry> = rows.into_iter().map(|(entry, _)| entry).collect();
        // Anchored where the stage is about to stand aside to, not where the
        // stage is now: the menu is opened by a press and the stage only
        // starts moving on the next frame, so anchoring on the current
        // rectangle would put the panel wherever the picture happened to be.
        // Taken from the window for the same reason — on the first frame of
        // all, the stage has not been measured even once.
        let anchor = self.menu_anchor();
        // Without this every menu is one row tall with an arrow under it:
        // `open_at` raises a window of nought to one rather than leaving it
        // for the renderer to fit, so the number of rows is the caller's to
        // say. As many as there are, up to as many as the window holds.
        let scale = lxb_toolkit::metrics::scale_for(self.window[1].max(1.0));
        let row = lxb_toolkit::metrics::Metric::RowHeight.value() * scale;
        let holds = ((self.window[1] * 0.72) / row.max(1.0)).floor() as usize;
        let window = entries.len().clamp(1, holds.max(1));
        self.asked = Some(Asked {
            title,
            entries,
            anchor,
            window,
        });
    }

    /// Raise the menu that was asked for, with its title cut to the panel.
    ///
    /// **Why this exists.** `lxb-render` lays a label out at the width of the
    /// row it is on only when that row can hold more than one line. A title is
    /// one line, so it is shaped unbounded and drawn unclipped, and a picture
    /// named anything longer than a menu is wide is written straight out
    /// through both sides of the panel. The panel is a known width before it
    /// is opened, so the name is cut to it here.
    ///
    /// `measure` is `Ui::measure`, handed in rather than reached for: nothing
    /// else in this file draws, and a test can answer it with arithmetic.
    pub fn settle_menu(&mut self, measure: &mut dyn FnMut(Text, &str) -> f32) {
        let Some(Asked {
            title,
            entries,
            anchor,
            window,
        }) = self.asked.take()
        else {
            return;
        };
        let scale = lxb_toolkit::metrics::scale_for(self.window[1].max(1.0));
        // The panel, less its margin and the padding a row's words start
        // after, which is exactly what the toolkit gives the title.
        let room = (menu::panel_width(0.0) - (menu::MARGIN + menu::LABEL_PADDING) * 2.0) * scale;
        let title = cut_to_fit(measure, Text::Title, &title, room);
        self.menu.open_at(anchor, Some(title), entries);
        self.menu.set_window(window);
    }

    fn menu_rows(&self) -> (String, Vec<(MenuEntry, Command)>) {
        let mut rows: Vec<(MenuEntry, Command)> = Vec::new();
        if self.mode == Mode::Viewer {
            rows.push((
                MenuEntry::new(crate::i18n::text("turn-right")).glyph("setting-rotation-90"),
                Command::Turn(1),
            ));
            rows.push((
                MenuEntry::new(crate::i18n::text("turn-left")).glyph("setting-rotation-270"),
                Command::Turn(-1),
            ));
            rows.push((
                MenuEntry::new(crate::i18n::text("fit-to-the-screen"))
                    .glyph("setting-scale")
                    .group(1),
                Command::ZoomTo(Zoom::Fit),
            ));
            rows.push((
                MenuEntry::new(crate::i18n::text("actual-size")).glyph("setting-resolution"),
                Command::ZoomTo(Zoom::Actual(1.0)),
            ));
            rows.push((
                MenuEntry::new(if self.info {
                    crate::i18n::text("hide-the-details")
                } else {
                    crate::i18n::text("show-the-details")
                })
                .glyph("setting-info")
                .group(2),
                Command::Info,
            ));
            rows.push((
                MenuEntry::new(if self.slideshow {
                    crate::i18n::text("stop-the-slideshow")
                } else {
                    crate::i18n::text("play-a-slideshow")
                })
                .glyph(if self.slideshow {
                    "media-pause"
                } else {
                    "media-play"
                }),
                Command::Slideshow,
            ));
            rows.push((
                MenuEntry::new(crate::i18n::text("back-to-the-folder"))
                    .glyph("category-images")
                    .group(3),
                Command::BackToGrid,
            ));
            return (
                self.current()
                    .map(|entry| entry.name.clone())
                    .unwrap_or_default(),
                rows,
            );
        }

        // The order the listing is already in wears the tick. Only that one:
        // `aside` takes the name of a real mark, and a row that is not the
        // current order is a row with nothing beside it rather than a row with
        // an empty mark.
        for order in Order::ALL {
            let row = MenuEntry::new(order.label()).glyph("setting-order");
            let row = if order == self.order {
                row.aside("chosen")
            } else {
                row
            };
            rows.push((row, Command::SortBy(order)));
        }
        rows.push((
            MenuEntry::new(if self.hidden {
                crate::i18n::text("hide-hidden-files")
            } else {
                crate::i18n::text("show-hidden-files")
            })
            .glyph("setting-typed")
            .group(1),
            Command::ShowHidden,
        ));
        rows.push((
            MenuEntry::new(crate::i18n::text("open-another-folder"))
                .glyph("file-folder")
                .group(2),
            Command::OpenFolder,
        ));
        rows.push((
            MenuEntry::new(crate::i18n::text("up-a-folder")).glyph("arrow-up"),
            Command::UpAFolder,
        ));
        (String::from(crate::i18n::text("options")), rows)
    }

    // ---- how the picture sits on the stage --------------------------------

    /// The size of the picture as it is on the file, in the file's own
    /// orientation. Answers nothing while it is still being read.
    ///
    /// **Not** turned. The turn is the shader's, about the quad's own centre,
    /// so the quad has to be the shape the picture really is; what the turn
    /// changes is how much room that quad takes up on the screen, which is
    /// [`View::footprint`].
    pub fn picture_size(&self, photos: &crate::photo::Photos) -> Option<(f32, f32)> {
        let path = self.photograph()?;
        let (width, height) = photos.size(path)?;
        Some((width as f32, height as f32))
    }

    /// How much room a picture of this size takes once it is turned.
    ///
    /// The bounding box of the turned rectangle, taken at whatever angle the
    /// turn has actually reached rather than at the one it is heading for —
    /// which is what keeps a picture inside the stage all the way through a
    /// turn instead of having its corners cut off half way round.
    fn footprint(&self, width: f32, height: f32) -> (f32, f32) {
        let (sin, cos) = (self.turn.sin().abs(), self.turn.cos().abs());
        (width * cos + height * sin, width * sin + height * cos)
    }

    /// Whether the picture is larger than the stage along an axis, which is
    /// what decides whether a direction pans or steps.
    fn overflows(&self, axis: usize) -> bool {
        self.overflow[axis] > 0.5
    }

    pub fn placement_now(&self) -> crate::photo::Placement {
        // The pan goes with the picture on the way back to its card: an offset
        // that stayed while the picture shrank would land it beside the card
        // rather than on it. One outside a crossing, so nothing else notices.
        let held = self
            .grown()
            .max(if self.going == Going::In { 1.0 } else { 0.0 });
        let stage = self.stepped_back(self.stage);
        let shrunk = if self.stage[2] > 0.0 {
            stage[2] / self.stage[2]
        } else {
            1.0
        };
        crate::photo::Placement {
            centre: [
                stage[0] + stage[2] * 0.5 + self.pan[0] * held * shrunk,
                stage[1] + stage[3] * 0.5 + self.pan[1] * held * shrunk,
            ],
            half: [self.quad[0] * 0.5 * shrunk, self.quad[1] * 0.5 * shrunk],
            turn: self.turn,
            opacity: self.showing,
            dim: self.dimmed(),
            radius: self.stage_radius() * shrunk,
            within: stage,
        }
    }

    // ---- opening out of a card, and going back into one -------------------

    /// How far the picture is between the card it was pressed on and the
    /// screen: 0 on the card, 1 filling the stage.
    ///
    /// **The one number a change of page is made of.** Everything that moves
    /// with it reads it — the stage itself, how round the picture's corners
    /// are, and how far the wall of pictures has stepped back behind it — so
    /// that all of them land on the same frame.
    pub fn grown(&self) -> f32 {
        match self.going {
            Going::Nowhere if self.mode == Mode::Viewer => 1.0,
            Going::Nowhere => 0.0,
            Going::In => motion::ease(1.0 - self.to_go),
            Going::Out => motion::ease(self.to_go),
        }
    }

    /// How far a change of page has got, or `None` once it has landed.
    ///
    /// `None` is what stops the wall of pictures being drawn under one for the
    /// rest of the evening: it is there to be flown out of and back into, and
    /// not otherwise.
    pub fn crossing(&self) -> Option<f32> {
        (self.going != Going::Nowhere).then(|| self.grown())
    }

    /// Whether the picture on the screen is on its way back into its card.
    ///
    /// Not `leaving`, which is the picture *before this one* on its way out
    /// as the folder is stepped through — a different animation entirely.
    pub fn going_back(&self) -> bool {
        self.going == Going::Out
    }

    /// How round the corners of whatever is on the stage are.
    ///
    /// The card's own rounding on the card and none at all on the screen. A
    /// picture that grew out of a card and reached the window still visibly a
    /// rounded card would have arrived as something else.
    pub fn stage_radius(&self) -> f32 {
        self.frame_radius * (1.0 - self.grown())
    }

    /// How solid whatever is on the stage is.
    ///
    /// What decides how much of the wall underneath is taken out from under it
    /// while a page is changing. A picture at half is a picture the names
    /// behind it should be half readable through.
    pub fn stage_solidity(&self) -> f32 {
        if self.standing_in().is_some() {
            return 1.0;
        }
        self.showing.clamp(0.0, 1.0)
    }

    /// The picture on the card this crossing runs to or from, as it stands on
    /// the screen this frame.
    ///
    /// The card's *picture*, not the card: a photograph that grew out of the
    /// whole card would jump on its first frame by the depth of its caption.
    fn crossing_card(&self, geometry: &Geometry) -> Option<[f32; 4]> {
        self.crossing()?;
        Some(geometry.frame_in(self.card_on_screen(geometry)?))
    }

    /// What a change of page turns about.
    ///
    /// The card, so that the one thing which does not move while the wall
    /// steps back behind the picture is the card the picture came out of.
    pub fn crossing_about(&self, geometry: &Geometry) -> [f32; 4] {
        self.crossing_card(geometry)
            .unwrap_or([0.0, 0.0, geometry.window[0], geometry.window[1]])
    }

    /// The picture whose thumbnail stands on the stage while the full-sized
    /// one is still being read, and the rectangle it stands in.
    ///
    /// Opening a photograph used to grow the card into an empty rectangle with
    /// the word *Reading…* in the middle of it. The thumbnail on the card is
    /// the picture that was pressed, so the card grows into itself and the
    /// full-sized picture comes up over it.
    ///
    /// **It is drawn at full strength, under the picture rather than crossing
    /// with it.** Two halves of the same photograph fading past each other add
    /// up to three quarters at the middle, and the quarter that is missing is
    /// the wall of pictures showing through the one being opened. Nothing is
    /// lost by holding it: what covers it is the same photograph at the same
    /// size, and sharper.
    ///
    pub fn standing_in(&self) -> Option<&Path> {
        if self.mode != Mode::Viewer || self.takes_over() || self.showing > 0.999 {
            return None;
        }
        self.photograph()
    }

    /// Where the thumbnail standing in on the stage goes, for a picture of
    /// this shape.
    ///
    /// **The rectangle travels from the card's own shape to the picture's.** A
    /// card crops its thumbnail to fill itself and the viewer shows the whole
    /// of it, so one of the two has to give: the crop unwinds as the picture
    /// grows, which makes the first frame the card exactly and the last the
    /// photograph exactly.
    ///
    /// The shape is asked of the thumbnail rather than read off `drawn`,
    /// which is nought until the full-sized picture has been read — and a
    /// rectangle that eased toward nothing and then jumped when the reader
    /// answered would be worse than not easing at all.
    pub fn standing_rect(&self, aspect: Option<f32>) -> [f32; 4] {
        let Some(aspect) = aspect.filter(|aspect| aspect.is_finite() && *aspect > 0.0) else {
            return self.stage;
        };
        let room = self.stage[2] / self.stage[3].max(0.001);
        let (across, down) = if aspect >= room {
            (self.stage[2], self.stage[2] / aspect)
        } else {
            (self.stage[3] * aspect, self.stage[3])
        };
        let fitted = [
            self.stage[0] + (self.stage[2] - across) * 0.5,
            self.stage[1] + (self.stage[3] - down) * 0.5,
            across,
            down,
        ];
        between(self.stage, fitted, self.grown())
    }
}

/// A rectangle part of the way between two others.
fn between(from: [f32; 4], to: [f32; 4], t: f32) -> [f32; 4] {
    let t = t.clamp(0.0, 1.0);
    let mut out = [0.0; 4];
    for (slot, (from, to)) in out.iter_mut().zip(from.iter().zip(&to)) {
        *slot = from + (to - from) * t;
    }
    out
}

/// Where everything on the page goes, worked out from the window alone.
///
/// One place, because the grid's cells are read three times a frame — to draw
/// them, to know how many columns a direction moves by, and to know which card
/// a photograph grows out of — and three answers would be three grids.
#[derive(Debug, Clone, Copy)]
pub struct Geometry {
    pub window: [f32; 2],
    pub margin: f32,
    pub head: f32,
    pub foot: f32,
    pub columns: usize,
    pub cell: [f32; 2],
    pub gap: f32,
    /// The air around the picture on a card, and the line of name under it.
    /// Kept because a photograph grows out of the picture it was pressed on
    /// rather than out of the card around it — see [`Geometry::frame_in`].
    card_inset: f32,
    card_caption: f32,
}

impl Geometry {
    pub fn of(window: [f32; 2], scale: impl Fn(f32) -> f32, gap: f32) -> Geometry {
        let margin = scale(48.0);
        let head = scale(86.0);
        let foot = scale(64.0);
        let across = (window[0] - margin * 2.0).max(1.0);
        // A card wide enough to recognise a photograph in and narrow enough
        // that a folder shows more than four. Rounded to whole columns, and
        // whatever is left over widens them all rather than being left at the
        // edge.
        let wanted = scale(300.0);
        let columns = (((across + gap) / (wanted + gap)).floor() as usize).clamp(1, 12);
        let width = (across - gap * (columns - 1) as f32) / columns as f32;
        Geometry {
            window,
            margin,
            head,
            foot,
            columns,
            // Room for the picture, and a line under it for the name.
            cell: [width, width * 0.68 + scale(34.0)],
            gap,
            card_inset: scale(10.0),
            card_caption: Text::Caption.on(window[1]) * Text::LINE,
        }
    }

    /// The picture on a card: the photograph itself, without the air around it
    /// or the line of name under it.
    ///
    /// **One sum, read twice.** `draw::card` puts the thumbnail here, and a
    /// photograph opening grows out of exactly this rectangle. Two answers
    /// would drift, and the drift would show as a picture jumping the moment
    /// it started moving.
    pub fn frame_in(&self, card: [f32; 4]) -> [f32; 4] {
        [
            card[0] + self.card_inset,
            card[1] + self.card_inset,
            (card[2] - self.card_inset * 2.0).max(1.0),
            (card[3] - self.card_inset * 2.0 - self.card_caption).max(1.0),
        ]
    }

    /// How round the picture on a card is.
    ///
    /// The card's own rounding less the air around the picture, which is what
    /// makes the two curves concentric — a picture inset inside a rounded
    /// corner by exactly the inset shares its centre of curvature. It was
    /// `Metric::TileRadius`, which is a *share* of a tile's size and not a
    /// number of pixels, so it came out as three tenths of one and the
    /// pictures were square inside rounded cards.
    ///
    /// Read by the card and by the photograph growing out of it, so that the
    /// two agree on the frame the animation starts on.
    pub fn frame_radius(&self) -> f32 {
        (Metric::CardRadius.on(self.window[1]) - self.card_inset).max(0.0)
    }

    pub fn viewport(&self) -> [f32; 4] {
        [
            self.margin,
            self.head,
            (self.window[0] - self.margin * 2.0).max(1.0),
            (self.window[1] - self.head - self.foot).max(1.0),
        ]
    }

    /// Where one card of the grid is, before the listing is scrolled.
    pub fn card(&self, index: usize) -> [f32; 4] {
        let row = (index / self.columns.max(1)) as f32;
        let column = (index % self.columns.max(1)) as f32;
        [
            self.margin + column * (self.cell[0] + self.gap),
            self.head + row * (self.cell[1] + self.gap),
            self.cell[0],
            self.cell[1],
        ]
    }

    pub fn rows(&self, count: usize) -> usize {
        count.div_ceil(self.columns.max(1))
    }

    /// How many rows fit in the viewport, as a fraction — a listing that shows
    /// two rows and a sliver of a third is showing 2.4 rows, and rounding that
    /// to two is what makes a page jump land short.
    pub fn visible_rows(&self) -> f32 {
        (self.viewport()[3] + self.gap) / (self.cell[1] + self.gap)
    }
}

impl View {
    /// Everything that has to be true before this frame acts or draws.
    ///
    /// Measure, then move. The order matters: a direction pressed this frame
    /// is answered against the picture as it is now, not as it was before the
    /// last stage animation finished.
    pub fn measure(&mut self, geometry: &Geometry, photos: &crate::photo::Photos) {
        self.window = geometry.window;
        self.columns = geometry.columns;
        self.visible_rows = geometry.visible_rows();
        self.frame_radius = geometry.frame_radius();

        let target = self.stage_target(geometry);
        if !self.stage_known {
            // Coming into the viewer, the stage starts as the card that was
            // pressed and grows into place. Every frame after this one is
            // `advance`'s.
            if self.mode == Mode::Viewer {
                self.stage = self.card_on_screen(geometry).unwrap_or(target);
            }
            self.stage_known = true;
        }

        self.picture = self.picture_size(photos).map_or([0.0; 2], |(w, h)| [w, h]);
        self.fit_the_picture();
    }

    /// How large the picture is drawn, for the stage as it stands now.
    ///
    /// Read twice a frame — once when the frame is measured and again once the
    /// stage has moved — because a stage that travels a tenth of the window in
    /// a frame and a size worked out before it moved is a picture drawn at last
    /// frame's size inside this frame's scissor, which shows as a hairline
    /// crawling round the edge of an opening picture.
    fn fit_the_picture(&mut self) {
        let [width, height] = self.picture;
        if width <= 0.0 || height <= 0.0 {
            self.fit = 0.0;
            self.drawn = [0.0; 2];
            self.quad = [0.0; 2];
            self.overflow = [0.0; 2];
            return;
        }
        let (across, down) = self.footprint(width, height);
        self.fit = (self.stage[2] / across).min(self.stage[3] / down);
        let wanted = match self.zoom {
            Zoom::Fit => self.fit,
            Zoom::Actual(scale) => scale,
        };
        if self.scale <= 0.0 || self.crossing().is_some() {
            // The first frame of a picture — it appears at the size it belongs
            // at rather than growing into it from nothing — and every frame of
            // a change of page, where the picture is *on* the stage rather
            // than following it. A spring would leave it a frame behind a
            // rectangle travelling a tenth of the window a frame, which reads
            // as a picture that does not quite fit the card it is landing on.
            self.scale = wanted;
            self.scale_speed = 0.0;
        }
        // The quad is the picture's own shape — the shader turns it — and what
        // the stage is measured against is the room that turned quad takes.
        self.quad = [width * self.scale, height * self.scale];
        self.drawn = [across * self.scale, down * self.scale];
        self.overflow = [
            (self.drawn[0] - self.stage[2]).max(0.0),
            (self.drawn[1] - self.stage[3]).max(0.0),
        ];
        self.wanted_scale = wanted;
    }

    /// Where the picture is allowed to be, once everything else has had its
    /// share of the window.
    fn stage_target(&self, geometry: &Geometry) -> [f32; 4] {
        let mut stage = geometry.viewport();
        // **The details and the menu are both down the right-hand side, so the
        // stage gives up the larger of the two rather than the sum.** Two
        // bites of one edge left the photograph at under a third of the width
        // of the window with both open — reported as a picture becoming
        // absurdly small when a menu was raised over the details.
        //
        // Both are drawn by the toolkit and therefore *under* the picture, so
        // the stage has to stand clear of them rather than let them be
        // covered. Both numbers are eased — `info_out` by this file and
        // `travelled` by the toolkit — which is what lets the stage be set
        // rather than sprung at, and is why the picture's edge and the pane's
        // can no longer come apart.
        let details = (self.info_width(geometry) + geometry.gap) * self.info_out;
        let menu = self.menu_room() * self.menu.travelled().clamp(0.0, 1.0);
        stage[2] = (stage[2] - details.max(menu).min(stage[2] * 0.6)).max(1.0);
        // A slideshow is the picture and nothing else.
        let whole = [0.0, 0.0, geometry.window[0], geometry.window[1]];
        between(stage, whole, self.slideshow_out)
    }

    /// Where the Options menu is anchored.
    ///
    /// **One sum, read twice**: by the press that raises the menu, and by the
    /// arithmetic that steps the picture back behind it. Taken from the window
    /// rather than the stage because the menu is opened by a press and the
    /// stage only starts moving on the next frame — and on the first frame of
    /// all, the stage has not been measured even once.
    fn menu_anchor(&self) -> [f32; 4] {
        [
            self.window[0] - self.menu_room(),
            self.window[1] * 0.5,
            0.0,
            0.0,
        ]
    }

    /// A rectangle as it stands once a menu has pushed the page back.
    ///
    /// `Ui::context_menu` calls `Ui::recede` on everything the toolkit drew.
    /// The photograph is not one of those things — it is drawn in a pass of
    /// its own, after them — so it is done to it here, by the same arithmetic,
    /// or the picture and the page it is on come apart by a tenth of the
    /// window every time somebody presses Options.
    pub fn stepped_back(&self, rect: [f32; 4]) -> [f32; 4] {
        let out = self.menu.travelled().clamp(0.0, 1.0);
        if out <= 0.0 {
            return rect;
        }
        let factor = 1.0 - menu::DEPTH * out;
        let about = self.menu_anchor();
        let offset = [
            (about[0] + about[2] * 0.5) * (1.0 - factor),
            (about[1] + about[3] * 0.5) * (1.0 - factor),
        ];
        [
            rect[0] * factor + offset[0],
            rect[1] * factor + offset[1],
            rect[2] * factor,
            rect[3] * factor,
        ]
    }

    /// How much light is left in the page while a menu is opening over it.
    ///
    /// The same reason as [`View::stepped_back`]: the toolkit dims what it
    /// drew, and a photograph that stayed bright while the page behind it went
    /// dark would be the one surface that had not heard.
    pub fn dimmed(&self) -> f32 {
        1.0 - (1.0 - menu::DIM) * self.menu.travelled().clamp(0.0, 1.0)
    }

    /// How much of the window a menu is given, which is both where it is
    /// anchored and how much the stage gives up. One number, because two would
    /// be a picture with a menu over the end of it.
    fn menu_room(&self) -> f32 {
        self.window[0] * 0.42
    }

    /// Where the details pane stands, for however much of it is out.
    ///
    /// **One sum, read twice**: `draw::details` puts the pane here and
    /// [`View::stage_target`] stands clear of it. Two answers drift, and the
    /// drift is a photograph drawn over a pane nobody can read.
    pub fn details_pane(&self, geometry: &Geometry) -> [f32; 4] {
        let width = self.info_width(geometry);
        [
            geometry.window[0] - (geometry.margin + width) * self.info_out,
            geometry.head,
            width,
            (geometry.window[1] - geometry.head - geometry.foot).max(1.0),
        ]
    }

    pub fn info_width(&self, geometry: &Geometry) -> f32 {
        (geometry.window[0] * 0.28).clamp(geometry.margin * 4.0, geometry.window[0] * 0.4)
    }

    /// The card the light is on, where it is on the screen this frame.
    fn card_on_screen(&self, geometry: &Geometry) -> Option<[f32; 4]> {
        if self.folder.entries.is_empty() {
            return None;
        }
        let mut card = geometry.card(self.cursor.min(self.folder.entries.len() - 1));
        card[1] -= self.scroll * (geometry.cell[1] + geometry.gap);
        Some(card)
    }

    /// Move everything that moves.
    pub fn advance(&mut self, dt: f32, geometry: &Geometry) {
        self.menu.advance(dt);
        self.dialog.advance(dt);
        self.files.advance(dt);
        self.pressing.advance(dt);

        // **Everything the stage is worked out from is wound on before the
        // stage is.** They used to be eased at the foot of this function, a
        // frame after the stage had read them and the same frame `draw` reads
        // them — which is a details pane a twelfth of its own width ahead of
        // the picture that has to stand clear of it, every frame of the
        // animation and worst at the end of it.
        self.slideshow_out = toward(
            self.slideshow_out,
            if self.slideshow && self.mode == Mode::Viewer && !self.overlaid() {
                1.0
            } else {
                0.0
            },
            dt / motion::duration::PANEL,
        );
        self.info_out = toward(
            self.info_out,
            if self.info && self.mode == Mode::Viewer {
                1.0
            } else {
                0.0
            },
            dt / motion::duration::PANEL,
        );
        // The crossing's own clock, wound down before anything reads it so
        // that everything this frame agrees about where it has got to.
        if self.going != Going::Nowhere {
            self.to_go = (self.to_go - dt / motion::duration::LAUNCH_OPEN).max(0.0);
        }

        let target = self.stage_target(geometry);
        match self.crossing_card(geometry) {
            // A change of page **scales** the picture between the card and the
            // screen rather than springing at it. The wall stepping back
            // behind it is tied to the same number, and a spring of its own
            // would put the two on two clocks.
            Some(card) => {
                if self.going == Going::In {
                    self.crossing_far = if self.crossing_far[2] > 0.0 {
                        between(self.crossing_far, target, dt / motion::duration::PANEL)
                    } else {
                        target
                    };
                }
                self.stage = between(card, self.crossing_far, self.grown());
            }
            None => self.stage = target,
        }

        self.fit_the_picture();

        if self.wanted_scale > 0.0 {
            let (at, moving) = spring(
                self.scale as f64,
                self.scale_speed as f64,
                self.wanted_scale as f64,
                motion::HIGHLIGHT_SPRING,
                dt as f64,
            );
            self.scale = at as f32;
            self.scale_speed = moving as f32;
        }

        let (turn, turning) = spring(
            self.turn as f64,
            self.turn_speed as f64,
            f64::from(self.quarters) * std::f64::consts::FRAC_PI_2,
            motion::HIGHLIGHT_SPRING,
            dt as f64,
        );
        self.turn = turn as f32;
        self.turn_speed = turning as f32;

        // A trigger is held rather than pressed, so it is worth a little more
        // zoom every frame for as long as it is down.
        if self.pull != 0.0 && self.mode == Mode::Viewer && !self.overlaid() {
            self.zoom_by(PULL_RATE.powf(self.pull * dt), None);
        }

        self.pan_step(dt);

        // Landed — *after* the frame it lands on has been laid out, or the
        // picture would be handed back to the spring on the very frame it was
        // due to arrive on its card, and spend that frame flying off it.
        if self.going != Going::Nowhere && self.to_go <= 0.0 {
            if self.going_back() {
                // What is on the card is the card's own thumbnail now.
                self.showing = 0.0;
            }
            self.going = Going::Nowhere;
        }

        if self.going_back() {
            // A picture shrinking back into its card is still on the screen,
            // and dissolves into the card's own thumbnail as it lands. See
            // [`LANDS_OVER`]. Never *more* than there was at the press: one
            // that was never read has nothing to land.
            self.showing = self.showing.min((self.to_go / LANDS_OVER).min(1.0));
        } else {
            let showing = if self.mode == Mode::Viewer && !self.takes_over() {
                1.0
            } else {
                0.0
            };
            // The two halves of a step are one animation and share its clock:
            // the picture going away and the one arriving over it. Everything
            // else this number does — a dialog taking the screen, the viewer
            // being left — is a panel's business and keeps a panel's speed.
            let over = if self.leaving.is_some() {
                CROSSFADE
            } else {
                motion::duration::PANEL
            };
            self.showing = toward(self.showing, showing, dt / over);
        }
        if self.leaving.is_some() {
            self.leaving_out = toward(self.leaving_out, 0.0, dt / CROSSFADE);
            if self.leaving_out <= 0.001 {
                self.leaving = None;
            }
        }

        if self.slideshow && self.mode == Mode::Viewer && !self.overlaid() {
            self.slide_left -= dt;
            if self.slide_left <= 0.0 {
                self.slide_left = SLIDE;
                if self.step(1).is_none() {
                    self.slideshow = false;
                }
            }
        }

        self.scroll_step(dt, geometry);
    }

    /// Panning, on a ramp.
    ///
    /// A direction arrives as one action and then as repeats of it, which is
    /// a series of presses rather than a held stick — the toolkit deliberately
    /// gives every control the same cadence. So a hold is recognised here as
    /// "another one arrived recently", and what it drives is a speed rather
    /// than a step, which is what makes panning smooth rather than notched.
    fn pan_step(&mut self, dt: f32) {
        for axis in 0..2 {
            self.since_push[axis] = (self.since_push[axis] + dt).min(f32::MAX / 4.0);
            if self.since_push[axis] > PAN_PATIENCE {
                self.push[axis] = 0.0;
                self.pushed_for[axis] = 0.0;
            }
            let target = if self.push[axis] == 0.0 {
                0.0
            } else {
                self.pushed_for[axis] += dt;
                let through = (self.pushed_for[axis] / PAN_RAMP).clamp(0.0, 1.0);
                self.push[axis] * (PAN_FROM + (PAN_TO - PAN_FROM) * through * through)
            };
            // Eased rather than set, so a pan starts and stops without a jerk
            // at either end.
            self.pan_speed[axis] += (target - self.pan_speed[axis]) * (1.0 - (-dt * 14.0).exp());
            self.pan[axis] -= self.pan_speed[axis] * dt;
        }
        self.clamp_pan();
    }

    /// Keep the picture's edges outside the stage where it is larger, and dead
    /// centre where it is not.
    ///
    /// Without this a pan runs on for ever and the photograph leaves the
    /// screen, which is a state nothing on the page could get out of.
    fn clamp_pan(&mut self) {
        for axis in 0..2 {
            let room = self.overflow[axis] * 0.5;
            if room <= 0.0 {
                self.pan[axis] = 0.0;
                self.pan_speed[axis] = 0.0;
                continue;
            }
            let held = self.pan[axis].clamp(-room, room);
            if held != self.pan[axis] {
                self.pan_speed[axis] = 0.0;
            }
            self.pan[axis] = held;
        }
    }

    /// Keep the light's row on the screen.
    fn scroll_step(&mut self, dt: f32, geometry: &Geometry) {
        let rows = geometry.rows(self.folder.entries.len()) as f32;
        let showing = geometry.visible_rows();
        let most = (rows - showing).max(0.0);
        let row = (self.cursor / geometry.columns.max(1)) as f32;
        let mut target = self.scroll;
        if row < target {
            target = row;
        } else if row > target + showing - 1.0 {
            target = row - showing + 1.0;
        }
        target = target.clamp(0.0, most);
        let (at, moving) = spring(
            self.scroll as f64,
            self.scroll_speed as f64,
            target as f64,
            motion::HIGHLIGHT_SPRING,
            dt as f64,
        );
        self.scroll = at as f32;
        self.scroll_speed = moving as f32;
    }

    /// A click somewhere on the page.
    pub fn press_at(&mut self, spot: Spot) -> Option<Sound> {
        match spot {
            Spot::Control(id) if id >= CARD => {
                let index = (id - CARD) as usize;
                if index >= self.folder.entries.len() {
                    return None;
                }
                // Two steps, as everywhere else in this language: the first
                // click carries the light, the second acts.
                if self.cursor != index {
                    self.cursor = index;
                    return Some(Sound::Move);
                }
                self.enter()
            }
            Spot::Control(STAGE) => self.press_stage(),
            _ => None,
        }
    }

    /// Pointing at something, which moves the light without acting.
    pub fn point_at(&mut self, spot: Spot) -> bool {
        if self.menu.is_open() {
            return self.menu.point_at(spot);
        }
        if self.dialog.is_open() {
            return self.dialog.point_at(spot);
        }
        false
    }
}

/// Ease a number towards another by a fraction of the way, per frame.
fn toward(from: f32, to: f32, step: f32) -> f32 {
    if (to - from).abs() <= 0.001 {
        return to;
    }
    from + (to - from) * step.clamp(0.0, 1.0)
}

/// What to show when the application is handed a path.
///
/// A folder opens as a folder. A photograph opens as that photograph, with its
/// own folder behind it — so that closing it lands on the grid it came from
/// rather than on nothing, and the arrows step through its neighbours.
/// Shorten a name until it fits, taking the middle out of it.
///
/// Two thirds from the front and a third from the back: the front is where a
/// name says what kind of thing it is, and the back is where it says which
/// one.
///
/// `measure` is handed in rather than reached for. Nothing in this file draws,
/// and the two callers measure with different things — `draw.rs` with the `Ui`
/// it is drawing into, and a test with arithmetic.
pub fn cut_to_fit(
    measure: &mut dyn FnMut(Text, &str) -> f32,
    text: Text,
    name: &str,
    room: f32,
) -> String {
    if room <= 0.0 || measure(text, name) <= room {
        return name.to_string();
    }
    let letters: Vec<char> = name.chars().collect();
    let mut keep = letters.len();
    while keep > 1 {
        keep -= 1;
        // Never more front than there are letters left to give. Rounding up
        // crosses over at two, and a `usize` that goes below nought does not
        // come back — it wraps, and the slice that is taken with it panics.
        let front = (keep.div_ceil(3) * 2).min(keep);
        let back = keep - front;
        let shorter: String = letters[..front]
            .iter()
            .chain(std::iter::once(&'…'))
            .chain(letters[letters.len() - back..].iter())
            .collect();
        if measure(text, &shorter) <= room {
            return shorter;
        }
    }
    String::from("…")
}

fn open_at(at: &Path, order: Order, hidden: bool) -> (Folder, usize, Mode) {
    if at.is_dir() {
        return (Folder::read(at, order, hidden), 0, Mode::Grid);
    }
    let Some(parent) = at.parent() else {
        return (Folder::read(at, order, hidden), 0, Mode::Grid);
    };
    let folder = Folder::read(parent, order, hidden);
    match folder.position(at) {
        Some(at) => (folder, at, Mode::Viewer),
        None => (folder, 0, Mode::Grid),
    }
}

impl View {
    /// The keyboard's own shorthands reach the same decisions the buttons do,
    /// rather than a second copy of them.
    pub fn command(&mut self, command: Command) -> Option<Sound> {
        self.run(command).or(Some(Sound::Press))
    }

    pub fn step_photo(&mut self, delta: isize) -> Option<Sound> {
        if self.mode != Mode::Viewer {
            return None;
        }
        self.step(delta)
    }

    /// One stop up or down the zoom ladder.
    ///
    /// The same ladder a press walks round, so `+` and the bottom face button
    /// cannot end up at different sizes — and measured against the scale the
    /// picture is really at, so it also follows a wheel.
    pub fn zoom_step(&mut self, delta: isize) -> Option<Sound> {
        if self.mode != Mode::Viewer {
            return None;
        }
        let stops = self.ladder();
        if stops.is_empty() {
            return None;
        }
        let now = self.effective_scale();
        let to = if delta > 0 {
            stops.iter().find(|stop| self.scale_of(**stop) > now * 1.02)
        } else {
            stops
                .iter()
                .rev()
                .find(|stop| self.scale_of(**stop) < now / 1.02)
        };
        let Some(to) = to.copied() else {
            return Some(Sound::Error);
        };
        self.settle_on(to);
        Some(Sound::Press)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{Entry, Kind};

    /// Every letter is the same width, which is all a cut needs to be tested.
    fn ruler(text: Text, string: &str) -> f32 {
        let _ = text;
        string.chars().count() as f32 * 10.0
    }

    #[test]
    fn a_name_too_wide_loses_its_middle_and_keeps_both_ends() {
        let cut = cut_to_fit(&mut ruler, Text::Body, "Holiday 2026 — 0431.jpeg", 120.0);
        assert!(cut.chars().count() < "Holiday 2026 — 0431.jpeg".chars().count());
        assert!(ruler(Text::Body, &cut) <= 120.0);
        assert!(cut.starts_with("Holi"), "the front says what this is");
        assert!(cut.ends_with("peg"), "and the back which one: {cut}");
    }

    /// A room too small for even two letters used to take a `usize` below
    /// nought and panic on the slice it wrapped into.
    #[test]
    fn a_name_with_no_room_at_all_is_an_ellipsis_rather_than_a_panic() {
        assert_eq!(cut_to_fit(&mut ruler, Text::Body, "Photo.jpg", 5.0), "…");
        assert_eq!(
            cut_to_fit(&mut ruler, Text::Body, "Photo.jpg", 0.0),
            "Photo.jpg"
        );
    }

    /// The defect this is for: `lxb-render` shapes a one-line label unbounded
    /// and draws it unclipped, so a picture named longer than a menu is wide
    /// is written out through both sides of the panel.
    #[test]
    fn a_menu_title_is_cut_to_the_panel_before_it_is_raised() {
        let mut view = looking_at(1.0);
        view.folder.entries = vec![Entry {
            path: PathBuf::from("/f/a.jpg"),
            name: String::from(
                "A picture with a name far longer than any menu is ever going to be wide.jpeg",
            ),
            kind: Kind::Photograph,
            bytes: 0,
            changed: None,
        }];
        view.cursor = 0;
        view.window = [1600.0, 900.0];
        view.open_menu();
        assert!(!view.menu.is_open(), "asked for, not yet raised");
        view.settle_menu(&mut ruler);
        assert!(view.menu.is_open());

        let scale = lxb_toolkit::metrics::scale_for(900.0);
        let room = (menu::panel_width(0.0) - (menu::MARGIN + menu::LABEL_PADDING) * 2.0) * scale;
        // The title is not readable back off the menu, so the same cut is
        // asked for again: what matters is that it is a cut at all and that
        // what comes out fits the panel.
        let cut = cut_to_fit(&mut ruler, Text::Title, &view.folder.entries[0].name, room);
        assert!(cut.ends_with("peg"));
        assert!(ruler(Text::Title, &cut) <= room);
    }

    /// A view with a picture measured onto a stage, without a GPU to measure
    /// it with. Everything `zoom_by` reads is set here by hand.
    fn looking_at(fit: f32) -> View {
        let mut view = View::new(Path::new("/nonexistent"), Order::Name, false);
        view.mode = Mode::Viewer;
        // As if a file manager had handed over one picture, which is what
        // `View::new` sets when it is given a path to a file rather than a
        // folder. The path above is neither, so it is set here.
        view.opened_on_a_picture = true;
        view.stage = [0.0, 0.0, 1000.0, 600.0];
        view.fit = fit;
        view.zoom = Zoom::Fit;
        view.scale = fit;
        view.quad = [1600.0 * fit, 1200.0 * fit];
        view.drawn = view.quad;
        view.overflow = [
            (view.drawn[0] - view.stage[2]).max(0.0),
            (view.drawn[1] - view.stage[3]).max(0.0),
        ];
        view
    }

    fn photograph(name: &str) -> Entry {
        Entry {
            path: PathBuf::from(name),
            name: name.to_string(),
            kind: Kind::Photograph,
            bytes: 0,
            changed: None,
        }
    }

    /// A wall of pictures to fly out of and back into.
    fn browsing() -> (View, Geometry) {
        let mut view = looking_at(1.0);
        view.mode = Mode::Grid;
        view.opened_on_a_picture = false;
        view.folder.entries = (0..4)
            .map(|number| photograph(&format!("{number}.jpg")))
            .collect();
        let geometry = Geometry::of([1600.0, 900.0], |value| value, 12.0);
        (view, geometry)
    }

    /// The whole of an opening in one number, and both ends of it: the picture
    /// starts on the card that was pressed and finishes on the page.
    #[test]
    fn a_picture_opens_out_of_the_card_it_was_pressed_on() {
        let (mut view, geometry) = browsing();
        view.cursor = 2;
        let card = geometry.frame_in(geometry.card(2));
        view.act(Action::Accept);
        assert_eq!(view.mode, Mode::Viewer, "the press opened it");
        assert_eq!(view.grown(), 0.0, "and it has not moved yet");

        view.advance(1.0 / 600.0, &geometry);
        for (was, is) in card.iter().zip(&view.stage) {
            assert!(
                (was - is).abs() < 1.0,
                "the first frame is the card: {card:?} against {:?}",
                view.stage
            );
        }

        let mut frames = 0;
        while view.crossing().is_some() && frames < 600 {
            let grown = view.grown();
            view.advance(1.0 / 60.0, &geometry);
            assert!(view.grown() >= grown, "it never goes backwards");
            frames += 1;
        }
        assert!(frames > 4, "it took a moment: {frames} frames");
        assert_eq!(view.grown(), 1.0, "and arrived");
        // Exactly where the stage would have sprung to on its own, so that the
        // last frame of the animation is the first frame of the page and there
        // is nothing left over to settle.
        let target = view.stage_target(&geometry);
        for (wanted, is) in target.iter().zip(&view.stage) {
            assert!(
                (wanted - is).abs() < 1.0,
                "it landed on {:?} rather than {target:?}",
                view.stage
            );
        }
    }

    /// And the way back, which is the same number run the other way — with the
    /// picture still on the screen for it. One let go at the press would have
    /// nothing to shrink.
    #[test]
    fn leaving_a_picture_puts_it_back_into_its_card() {
        let (mut view, geometry) = browsing();
        view.cursor = 1;
        view.act(Action::Accept);
        while view.crossing().is_some() {
            view.advance(1.0 / 60.0, &geometry);
        }

        // As if the picture had really been read, which no path in a test is.
        view.showing = 1.0;
        view.act(Action::Back);
        assert_eq!(view.mode, Mode::Grid, "the wall is the page again");
        assert!(view.going_back(), "and the picture is on its way back");
        view.advance(1.0 / 60.0, &geometry);
        assert!(
            view.showing > 0.9,
            "a picture that had gone would have nothing to shrink"
        );

        while view.crossing().is_some() {
            view.advance(1.0 / 60.0, &geometry);
        }
        assert_eq!(view.grown(), 0.0);
        assert_eq!(view.showing, 0.0, "gone by the time it lands");
        let card = geometry.frame_in(geometry.card(1));
        for (was, is) in card.iter().zip(&view.stage) {
            assert!((was - is).abs() < 1.0, "{card:?} against {:?}", view.stage);
        }
    }

    /// The card draws its thumbnail in one rectangle and the picture grows out
    /// of the same one. Two answers would drift, and the drift would show as a
    /// photograph jumping on the first frame of every opening.
    #[test]
    fn the_picture_on_a_card_is_one_rectangle() {
        let geometry = Geometry::of([1600.0, 900.0], |value| value, 12.0);
        let card = geometry.card(0);
        let frame = geometry.frame_in(card);
        assert!(frame[0] > card[0] && frame[1] > card[1], "inset on both");
        assert!(
            frame[1] + frame[3] < card[1] + card[3] - 1.0,
            "and a line of name left under it"
        );
        assert!(
            geometry.frame_radius() > 0.0,
            "the picture is rounded, less than the card around it"
        );
        assert!(geometry.frame_radius() < Metric::CardRadius.on(900.0));
    }

    /// Stepping through a folder is the one thing anybody does repeatedly
    /// here, so its crossfade is half again as fast as a panel's. Held to that
    /// by counting frames rather than by trusting the constant, because what
    /// decides it is the *clock the eased number is on* and not the number.
    #[test]
    fn one_picture_crosses_to_the_next_faster_than_a_panel_moves() {
        let geometry = Geometry::of([1600.0, 900.0], |value| value, 12.0);
        let count = |over: f32| {
            let mut left = 1.0f32;
            let mut frames = 0i32;
            while left > 0.001 && frames < 600 {
                left = toward(left, 0.0, (1.0 / 60.0) / over);
                frames += 1;
            }
            frames
        };
        let panel = count(motion::duration::PANEL);
        let crossing = count(CROSSFADE);
        assert!(
            (crossing as f32 * 1.5 - panel as f32).abs() <= 2.0,
            "{crossing} frames against a panel's {panel}"
        );

        // And the viewer really runs on it: two pictures, one step, and the
        // one going away is gone in that many frames.
        let mut view = looking_at(1.0);
        view.folder.entries = vec![photograph("/f/a.jpg"), photograph("/f/b.jpg")];
        view.cursor = 0;
        view.act(Action::Next);
        assert!(view.leaving.is_some(), "the one before is on its way out");
        let mut frames = 0i32;
        while view.leaving.is_some() && frames < 600 {
            view.advance(1.0 / 60.0, &geometry);
            frames += 1;
        }
        assert!(
            (frames - crossing).abs() <= 2,
            "it took {frames} frames rather than {crossing}"
        );
    }

    /// The defect the user reported: the picture was drawn over the details
    /// pane on the way in *and* on the way out. The pane arrived at the speed
    /// of a fade and the stage crawled after it on a spring, so for a fifth of
    /// a second the stage still covered the pane — and the picture is drawn in
    /// a pass after the toolkit's, so it went straight over the top.
    #[test]
    fn the_picture_never_reaches_the_details_pane() {
        let geometry = Geometry::of([1600.0, 900.0], |value| value, 12.0);
        let mut view = looking_at(1.0);
        view.window = geometry.window;
        view.info = true;
        for step in 0..=20 {
            view.info_out = step as f32 / 20.0;
            let stage = view.stage_target(&geometry);
            let pane = view.details_pane(&geometry);
            assert!(
                stage[0] + stage[2] <= pane[0] + 0.01,
                "at {}: the stage reaches {} and the pane starts at {}",
                view.info_out,
                stage[0] + stage[2],
                pane[0]
            );
        }
    }

    /// And the one that made it absurd: both panels are down the right-hand
    /// side, so the stage gives up the larger of the two and not the sum.
    #[test]
    fn a_menu_over_the_details_takes_one_bite_and_not_two() {
        let geometry = Geometry::of([1600.0, 900.0], |value| value, 12.0);
        let mut view = looking_at(1.0);
        view.window = geometry.window;
        view.info_out = 1.0;
        let details_only = view.stage_target(&geometry)[2];
        view.menu.open_at(
            view.menu_anchor(),
            None,
            vec![MenuEntry::new("Turn right"), MenuEntry::new("Turn left")],
        );
        for _ in 0..60 {
            view.menu.advance(1.0 / 60.0);
        }
        assert!(view.menu.travelled() > 0.99, "the menu is really open");
        let both = view.stage_target(&geometry)[2];
        assert!(both < details_only, "a menu does take room: {both}");
        assert!(
            both > geometry.viewport()[2] * 0.5,
            "but not both their rooms at once: {both} of {}",
            geometry.viewport()[2]
        );
    }

    /// The crop a card puts on a thumbnail unwinds as the picture grows, so
    /// that the first frame is the card and the last is the photograph.
    #[test]
    fn the_crop_a_card_puts_on_a_picture_unwinds() {
        let mut view = looking_at(1.0);
        view.stage = [0.0, 0.0, 1000.0, 600.0];
        // A tall picture on a wide stage: the two shapes disagree as much as
        // they ever will.
        let tall = Some(0.5);
        view.going = Going::In;
        view.to_go = 1.0;
        assert_eq!(
            view.standing_rect(tall),
            view.stage,
            "on the card it fills the card, exactly as the card draws it"
        );
        view.to_go = 0.0;
        let landed = view.standing_rect(tall);
        assert!((landed[2] / landed[3] - 0.5).abs() < 0.01, "{landed:?}");
        assert!(landed[3] <= view.stage[3] + 0.01, "and inside the stage");
        assert_eq!(
            view.standing_rect(None),
            view.stage,
            "a shape nobody knows yet is not a shape to ease towards"
        );
    }

    /// The one that was really wrong.
    ///
    /// A held trigger is a great many small zooms rather than one large one,
    /// so anything that rounds a small zoom away stops it dead. The snap back
    /// to `Fit` used to be wider than a frame's worth of trigger, and a
    /// trigger held for a second changed nothing whatsoever.
    #[test]
    fn a_held_trigger_accumulates() {
        let mut view = looking_at(0.5);
        // A second of it, at sixty frames.
        for _ in 0..60 {
            view.zoom_by(PULL_RATE.powf(1.0 / 60.0), None);
        }
        let reached = view.effective_scale();
        assert!(
            reached > 1.4 && reached < 1.6,
            "a second of full trigger should treble the zoom, reached {reached}"
        );
    }

    #[test]
    fn a_picture_is_never_taken_out_past_fitting() {
        let mut view = looking_at(0.5);
        for _ in 0..600 {
            view.zoom_by(PULL_RATE.powf(-1.0 / 60.0), None);
        }
        assert_eq!(view.zoom, Zoom::Fit, "zooming out lands on fitting");
        assert_eq!(view.effective_scale(), 0.5);
        assert_eq!(view.pan, [0.0; 2], "a picture that fits is centred");
    }

    #[test]
    fn a_wheel_keeps_the_point_under_the_pointer_still() {
        let mut view = looking_at(0.5);
        // Off the middle, so a mistake on either axis cannot cancel out — but
        // not so far that keeping it still would need the picture dragged off
        // its own edge, which is the other test below.
        let at = [600.0, 250.0];
        let middle = [500.0, 300.0];

        // Where in the picture that point is, before.
        let before = [
            (at[0] - middle[0] - view.pan[0]) / view.effective_scale(),
            (at[1] - middle[1] - view.pan[1]) / view.effective_scale(),
        ];
        view.zoom_by(NOTCH * NOTCH * NOTCH, Some(at));
        let after = [
            (at[0] - middle[0] - view.pan[0]) / view.effective_scale(),
            (at[1] - middle[1] - view.pan[1]) / view.effective_scale(),
        ];
        for axis in 0..2 {
            assert!(
                (before[axis] - after[axis]).abs() < 0.5,
                "the picture moved under the pointer on axis {axis}: \
                 {before:?} became {after:?}"
            );
        }
    }

    /// Keeping the point still is what the picture wants; staying on the
    /// screen is what it must do. A pointer near the edge asks for a pan that
    /// would show wallpaper past the picture's own edge, and the edge wins.
    #[test]
    fn but_never_by_dragging_the_picture_off_its_own_edge() {
        let mut view = looking_at(0.5);
        view.zoom_by(NOTCH * NOTCH * NOTCH, Some([1000.0, 300.0]));
        let room = view.overflow[0] * 0.5;
        assert!(room > 0.0, "the picture does hang over the edge now");
        assert!(
            view.pan[0].abs() <= room + 0.01,
            "panned {} with only {room} of room",
            view.pan[0]
        );
    }

    #[test]
    fn the_ladder_is_in_the_order_the_sizes_really_come_out_in() {
        // A photograph smaller than the stage fits at more than its own size,
        // so *fit* is not the first stop on it.
        let view = looking_at(3.0);
        let stops = view.ladder();
        let scales: Vec<f32> = stops.iter().map(|stop| view.scale_of(*stop)).collect();
        let mut sorted = scales.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(scales, sorted, "the stops come out smallest first");
        assert!(scales.contains(&3.0), "fitting is one of them");
    }

    #[test]
    fn a_stop_the_same_size_as_fitting_is_not_a_second_stop() {
        // Fitting at almost exactly one to one: pressing must not appear to
        // do nothing.
        let view = looking_at(1.005);
        assert_eq!(
            view.ladder().len(),
            3,
            "fit and actual size are one stop here, leaving three"
        );
    }

    #[test]
    fn a_press_goes_on_from_where_a_wheel_left_the_picture() {
        let mut view = looking_at(0.5);
        view.zoom_by(3.0, None); // 1.5x, between the 1x and 2x stops
        view.next_stop();
        assert_eq!(
            view.zoom,
            Zoom::Actual(2.0),
            "the next stop above where the picture actually is"
        );
    }

    /// A file manager's double-click: Back is the way out of the application,
    /// not the way into a folder nobody asked for.
    #[test]
    fn opened_on_one_picture_back_closes() {
        let mut view = looking_at(0.5);
        assert!(view.opened_on_a_picture, "started in the viewer");
        assert!(view.closes_on_back());
        view.act(Action::Back);
        assert!(view.quit, "Back closed it");
        assert_eq!(view.mode, Mode::Viewer, "and did not go to a folder first");
    }

    /// Until they say otherwise, which is what the menu row is for. From then
    /// on Back is the *folder's* Back — which walks up inside the walk, and
    /// only closes once it is back at the top of it.
    #[test]
    fn asking_for_the_folder_makes_it_an_ordinary_walk() {
        let mut view = looking_at(0.5);
        view.command(Command::BackToGrid);
        assert_eq!(view.mode, Mode::Grid);
        assert!(!view.opened_on_a_picture, "no longer a handed-over picture");

        // Somewhere inside the walk: Back walks up rather than closing.
        view.top = PathBuf::from("/one");
        view.folder.path = PathBuf::from("/one/two");
        assert!(!view.closes_on_back());
        view.act(Action::Back);
        assert!(
            !view.quit,
            "it walked out of the folder rather than closing"
        );
        assert_eq!(view.folder.path, PathBuf::from("/one"));
    }

    /// Opened on a folder, the viewer is somewhere inside the walk and Back
    /// goes back to it, exactly as before.
    #[test]
    fn opened_on_a_folder_back_returns_to_it() {
        let mut view = looking_at(0.5);
        view.opened_on_a_picture = false;
        view.act(Action::Back);
        assert!(!view.quit);
        assert_eq!(view.mode, Mode::Grid);
    }

    /// The folder handed over is the top of the walk: Back closes at it rather
    /// than carrying on up through the home directory and out to the disk.
    #[test]
    fn back_closes_at_the_folder_the_walk_was_opened_on() {
        let mut view = looking_at(0.5);
        view.mode = Mode::Grid;
        view.opened_on_a_picture = false;
        view.folder.path = PathBuf::from("/home/someone/Holiday");
        view.top = PathBuf::from("/home/someone/Holiday");
        assert!(view.closes_on_back());
        view.act(Action::Back);
        assert!(
            view.quit,
            "it closed rather than walking up to /home/someone"
        );
    }

    /// And walks up to it from anywhere inside it.
    #[test]
    fn back_walks_up_to_the_top_but_not_past_it() {
        let mut view = looking_at(0.5);
        view.mode = Mode::Grid;
        view.opened_on_a_picture = false;
        view.top = PathBuf::from("/home/someone/Holiday");
        view.folder.path = PathBuf::from("/home/someone/Holiday/Crete");
        assert!(!view.closes_on_back());
        view.act(Action::Back);
        assert!(!view.quit);
        assert_eq!(view.folder.path, PathBuf::from("/home/someone/Holiday"));
        // And now it is at the top.
        assert!(view.closes_on_back());
    }

    /// Deliberately stepping over the top moves the top, or Back would close
    /// from a folder further out than the one it closes at.
    #[test]
    fn asking_to_go_above_the_top_moves_the_top() {
        let mut view = looking_at(0.5);
        view.mode = Mode::Grid;
        view.opened_on_a_picture = false;
        view.folder.path = PathBuf::from("/home/someone/Holiday");
        view.top = PathBuf::from("/home/someone/Holiday");
        view.command(Command::UpAFolder);
        assert_eq!(view.top, PathBuf::from("/home/someone"));
        assert!(view.closes_on_back(), "the new folder is the new top");
    }

    #[test]
    fn a_folder_with_nothing_above_it_is_a_top_of_its_own() {
        let mut view = looking_at(0.5);
        view.mode = Mode::Grid;
        view.opened_on_a_picture = false;
        view.top = PathBuf::from("/somewhere/else");
        view.folder.path = PathBuf::from("/");
        assert!(view.closes_on_back());
    }

    #[test]
    fn a_wheel_in_the_grid_walks_the_grid_instead_of_zooming() {
        let mut view = looking_at(0.5);
        view.mode = Mode::Grid;
        view.columns = 1;
        view.folder.entries = (0..4)
            .map(|number| Entry {
                path: PathBuf::from(format!("/x/{number}.png")),
                name: format!("{number}.png"),
                kind: Kind::Photograph,
                bytes: 0,
                changed: None,
            })
            .collect();
        view.wheel(1, [10.0, 10.0]);
        assert_eq!(view.cursor, 1, "one notch is one row");
        assert_eq!(view.zoom, Zoom::Fit, "and nothing was zoomed");
    }
}
