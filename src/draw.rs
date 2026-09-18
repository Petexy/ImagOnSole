//! Putting the viewer on the screen.
//!
//! Everything here except the photograph itself is `lxb-render` answering for
//! the material — the glass of a card, the light that travels between them,
//! the marks, the words and their sizes. Not one colour, radius or duration is
//! named in this file.
//!
//! The photograph is the exception, and the return value is where it goes: a
//! rectangle handed back to `main.rs` for `photo.rs` to draw over the frame
//! once this one is composed. See the note at the top of `photo.rs`.

use std::path::PathBuf;

use lxb_render::{Align, Fit, Ui};
use lxb_toolkit::{
    control, material::Surface, metrics::Metric, palette::Role, settings::IconStyle,
    typography::Text,
};

use crate::facts;
use crate::legend::{self, hint, Button, Hint};
use crate::library::Kind;
use crate::photo::{Photos, Placement};
use crate::view::{Geometry, Mode, View, Zoom, CARD, SCROLL_BAR, STAGE};

/// What the buttons do, on each of the two pages.
///
/// Asked for by the same state that answers the press, so the row cannot come
/// to name something that is not there.
pub fn hints(view: &View) -> Vec<Hint> {
    match view.mode {
        Mode::Grid => {
            // Back walks out of the walk and closes at the top of it, so the
            // word changes with it rather than naming something that does not
            // happen.
            let mut hints = vec![
                hint(crate::i18n::text("options"), Button::Options),
                if view.closes_on_back() {
                    hint(crate::i18n::text("close"), Button::Back)
                } else {
                    hint(crate::i18n::text("back"), Button::Back)
                },
            ];
            if view.folder.photographs() > 0 {
                hints.insert(0, hint(crate::i18n::text("slideshow"), Button::Start));
            }
            if view.current().is_some() {
                hints.insert(
                    0,
                    hint(
                        if view.current().is_some_and(|entry| entry.is_folder()) {
                            crate::i18n::text("open")
                        } else {
                            crate::i18n::text("view")
                        },
                        Button::Accept,
                    ),
                );
            }
            hints
        }
        Mode::Viewer => vec![
            hint(crate::i18n::text("zoom"), Button::Accept),
            hint(crate::i18n::text("slideshow"), Button::Start),
            hint(crate::i18n::text("options"), Button::Options),
            // What the button really does, which is not always the same thing:
            // opened on one picture, there is nothing behind it to go back to
            // and Back closes. A legend that said "Back" there would be naming
            // something that does not happen.
            if view.closes_on_back() {
                hint(crate::i18n::text("close"), Button::Back)
            } else {
                hint(crate::i18n::text("back"), Button::Back)
            },
        ],
    }
}

pub fn draw(
    view: &mut View,
    ui: &mut Ui,
    geometry: &Geometry,
    photos: &mut Photos,
    pad: bool,
    icons: IconStyle,
) -> Vec<(PathBuf, Placement)> {
    let mut placements = Vec::new();

    // **The wall of pictures is drawn under a photograph that is growing out
    // of it or shrinking back into it**, stepping back and fading out as it
    // goes: the gesture a page makes when a menu opens over it, at the size of
    // a whole change of page. `Ui::recede` and `Ui::recede_behind` are the only
    // doors on to it, and both act on everything drawn so far — which is why
    // this happens *between* the two pages rather than inside either.
    let crossing = view.crossing();
    if view.mode == Mode::Grid || crossing.is_some() {
        grid(view, ui, geometry, icons);
    }
    if let Some(grown) = crossing.filter(|grown| *grown > 0.0) {
        // About the card, so that the one thing which does not move while the
        // wall steps back is the card the picture came out of.
        ui.recede(grown, view.crossing_about(geometry));
        // Faded to nothing rather than to the `menu::DIM` a page keeps behind
        // a panel: what stands over this one is not a panel but the whole of
        // the next page, and a wall left at four tenths would still be there
        // when the animation ended.
        //
        // **And the words cut out from under the picture**, which is the
        // second half of what this call is for. Every quad of a layer is drawn
        // before every word of it, so a name on a card is drawn over anything
        // the same page puts on top of it however late — and the picture
        // growing out of the card is exactly that.
        ui.recede_behind(view.stage, 1.0 - grown, view.stage_solidity());
    }

    if view.mode == Mode::Viewer {
        viewer(view, ui, geometry, photos, icons, &mut placements);
    } else if view.going_back() {
        // The picture is still on the screen on its way back into its card,
        // and the page it belonged to is not.
        if let Some(path) = view.photograph() {
            placements.push((path.to_path_buf(), view.placement_now()));
        }
    }

    // A slideshow is the picture and nothing else — the row of hints would be
    // the only thing on the screen that was not the photograph.
    if !(view.slideshow && view.mode == Mode::Viewer) {
        let hints = hints(view);
        let line = ui.line(Text::Caption);
        let middle = geometry.window[1] - geometry.foot * 0.5;
        let right = geometry.window[0] - geometry.margin;
        let left = legend::row(ui, right, middle, &hints, pad, icons);

        if let Some(note) = view.note.clone() {
            let width = (left - geometry.margin * 2.0).max(0.0);
            ui.label(
                [geometry.margin, middle - line * 0.5, width, line],
                Text::Caption,
                &note,
                Role::TextSoft,
                Align::Left,
            );
        }
    }

    // Last, and over everything: the panels the toolkit owns. Each is drawn in
    // its own layer and refracts what is already beneath it, which is why they
    // cannot be drawn before the page they are over.
    ui.context_menu(&mut view.menu);
    ui.dialog(&mut view.dialog);
    view.files.draw(ui);

    placements
}

// ---- the folder ---------------------------------------------------------

fn grid(view: &mut View, ui: &mut Ui, geometry: &Geometry, icons: IconStyle) {
    // The head is drawn below unless the viewer's own is standing in the same
    // band — a photograph opening out of a card is drawn under one, and two
    // marks and two names in one bar is not a change of page, it is both pages
    // at once. The words behind a panel are cut away by `recede_behind`; a
    // mark is a quad, and glass refracts what is behind it rather than hiding
    // it.
    if view.mode == Mode::Grid {
        let title = view
            .folder
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("/")
            .to_string();
        head(ui, geometry, "category-images", &title, &count(view), icons);
    }

    let viewport = geometry.viewport();
    let step = geometry.cell[1] + geometry.gap;
    let lifted = view.scroll * step;

    if view.folder.entries.is_empty() {
        let line = ui.line(Text::Body);
        ui.label(
            [
                viewport[0],
                viewport[1] + viewport[3] * 0.5 - line * 0.5,
                viewport[2],
                line,
            ],
            Text::Body,
            if view.folder.unreadable {
                crate::i18n::text("this-folder-cannot-be-opened")
            } else {
                crate::i18n::text("no-pictures-here")
            },
            Role::TextSoft,
            Align::Centre,
        );
        return;
    }

    // Everything the grid draws is cut to the viewport afterwards, so a row
    // half over the edge is drawn in half rather than left out or spilled over
    // the head.
    let from = ui.written();

    // The light goes down before the cards it is behind, and travels rather
    // than appearing — which is the whole of what makes a list of pictures
    // feel like one thing being moved over.
    let mut lit = geometry.card(view.cursor);
    lit[1] -= lifted;
    let dt = ui.dt();
    let strength = match view.pressing.through() {
        Some(through) => 1.0 - through * 0.35,
        None => 1.0,
    };
    let at = view.light.glide(lit, dt);
    // `Ui::selection` is shaped for a row — its radius is half the height,
    // which on a card as tall as it is wide is an ellipse. A card is lit to
    // its own corner instead, which is the same material at the same role.
    let radius = ui.m(Metric::CardRadius);
    ui.lit(at, radius, control::LIT_ROLE, at[2], strength);

    let first = (view.scroll.floor() as usize).saturating_sub(1) * geometry.columns.max(1);
    let last =
        ((view.scroll + geometry.visible_rows()).ceil() as usize + 1) * geometry.columns.max(1);
    for index in first..last.min(view.folder.entries.len()) {
        let mut rect = geometry.card(index);
        rect[1] -= lifted;
        // Nothing is drawn for a row that is nowhere near the window; a folder
        // of ten thousand would otherwise be ten thousand cards a frame.
        if rect[1] > viewport[1] + viewport[3] || rect[1] + rect[3] < viewport[1] {
            continue;
        }
        card(view, ui, geometry, index, rect, icons);
    }

    let to = ui.written();
    ui.cut_between(from, to, viewport);

    // The ends of the listing dissolve into what is behind the page rather
    // than stopping at a line. How much really continues past each end is what
    // decides how much of a fade there is.
    let rows = geometry.rows(view.folder.entries.len()) as f32;
    let most = (rows - geometry.visible_rows()).max(0.0);
    let band = geometry.cell[1] * 0.5;
    ui.soft_edges(
        viewport,
        band,
        (view.scroll / 0.6).clamp(0.0, 1.0),
        ((most - view.scroll) / 0.6).clamp(0.0, 1.0),
    );

    // A bar is for a hand holding a pointer. A pad has the light, which
    // already says where in the listing it is, and a bar it could not reach
    // would be a control on the screen that no button touches.
    if most > 0.0 {
        let width = ui.scroll_bar_width();
        let track = [
            viewport[0] + viewport[2] - width,
            viewport[1],
            width,
            viewport[3],
        ];
        let thumb = ui.scroll_bar(
            track,
            view.scroll / most.max(0.0001),
            (geometry.visible_rows() / rows).clamp(0.0, 1.0),
            false,
        );
        ui.spot(SCROLL_BAR, thumb);
    }
}

fn count(view: &View) -> String {
    let pictures = view.folder.photographs();
    let folders = view.folder.entries.len() - pictures;
    // Counts go to the catalog as numbers, never as text: the form of the noun
    // is the language's decision and it makes it by looking at the number.
    match (pictures, folders) {
        (0, 0) => String::new(),
        (0, folders) => crate::message!("count-folders", "count" => folders),
        (pictures, 0) => crate::message!("count-pictures", "count" => pictures),
        (pictures, folders) => format!(
            "{}  ·  {}",
            crate::message!("count-pictures", "count" => pictures),
            crate::message!("count-folders", "count" => folders)
        ),
    }
}

fn card(
    view: &View,
    ui: &mut Ui,
    geometry: &Geometry,
    index: usize,
    rect: [f32; 4],
    icons: IconStyle,
) {
    let entry = &view.folder.entries[index];
    let lit = index == view.cursor;
    let radius = ui.m(Metric::CardRadius);
    let press = view.pressing.state(lit);

    ui.spot(CARD + index as u32, rect);
    // **Only the chosen card wears the control.** A wall of pictures in which
    // every one of them is a button is a wall with nothing picked out: the
    // glass, the rim and the press read as decoration on all of them at once
    // and as selection on none. So the rest are the picture and its name, and
    // the one being looked at is the only thing on the page that looks like
    // something to press.
    //
    // The rectangle is the same either way, so nothing moves as the light
    // travels — only the press, which is what a press is for.
    let sunk = if lit {
        ui.control(rect, radius, press, rect[2], 1.0)
    } else {
        rect
    };

    let inset = ui.s(10.0);
    let line = ui.line(Text::Caption);
    // Asked for rather than worked out here: a photograph opening grows out of
    // this rectangle, and the two answers have to be one answer. See
    // `Geometry::frame_in`.
    let picture = geometry.frame_in(sunk);

    match entry.kind {
        Kind::Folder => {
            // The tile a photograph fills, with the mark on it. Not the
            // control — that belongs to whichever card is chosen — but a
            // folder with nothing behind it is a small mark adrift in a row of
            // solid rectangles, and the row stops reading as a row.
            ui.card(
                picture,
                Surface::Panel,
                Role::Glass,
                if lit { 0.5 } else { 0.35 },
            );
            let mark = ui.s(64.0).min(picture[3]);
            ui.icon_tinted(
                [
                    picture[0] + picture[2] * 0.5 - mark * 0.5,
                    picture[1] + picture[3] * 0.5 - mark * 0.5,
                    mark,
                    mark,
                ],
                "file-folder",
                icons,
                Role::Text,
                if lit { 1.0 } else { 0.8 },
            );
        }
        Kind::Photograph => {
            // A card is the one place a 512-pixel thumbnail is exactly the
            // right thing: it is the size the atlas holds and about the size
            // the card is. Cover rather than contain, so a wall of pictures is
            // a wall rather than a row of differently shaped holes.
            if !ui.picture(
                picture,
                geometry.frame_radius(),
                &entry.path,
                Fit::Cover,
                1.0,
            ) {
                // Not read yet, or not readable at all. Something has to hold
                // the card's shape either way, or the grid flickers as it
                // fills in.
                ui.card(picture, Surface::Panel, Role::Glass, 0.5);
            }
        }
    }

    let room = (sunk[2] - inset * 2.0).max(1.0);
    let name = fit_text(ui, Text::Caption, &entry.name, room);
    ui.label(
        [
            sunk[0] + inset,
            sunk[1] + sunk[3] - inset - line,
            room,
            line,
        ],
        Text::Caption,
        &name,
        if lit { Role::Text } else { Role::TextSoft },
        Align::Left,
    );
}

/// Shorten a name to fit, out of the middle.
///
/// **Nothing clips a word.** Every quad of a layer is drawn before any of that
/// layer's words, so a label wider than the rectangle it was given is not cut
/// off — it is written straight over whatever is beside it, which on a grid is
/// the next card's name. So anything that might not fit has to be cut before
/// it is asked for.
///
/// Out of the middle, because both ends of a file name carry something and the
/// middle rarely does: a folder of `Screenshot_20260824_142619.png` differs
/// from its neighbours only in the last few digits, and cutting the end would
/// leave a column of identical labels. The extension survives for the same
/// reason.
fn fit_text(ui: &mut Ui, text: Text, name: &str, room: f32) -> String {
    crate::view::cut_to_fit(
        &mut |text, string| ui.measure(text, string),
        text,
        name,
        room,
    )
}

// ---- one picture ---------------------------------------------------------

fn viewer(
    view: &mut View,
    ui: &mut Ui,
    geometry: &Geometry,
    photos: &mut Photos,
    icons: IconStyle,
    placements: &mut Vec<(PathBuf, Placement)>,
) {
    let Some(entry) = view.current().cloned() else {
        return;
    };

    if !view.slideshow {
        head(
            ui,
            geometry,
            "category-images",
            &entry.name,
            &position(view),
            icons,
        );
    }

    // The stage is a target a pointer can land on: a click on the picture is
    // the same press of the same button the legend names.
    ui.spot(STAGE, view.stage);

    if view.info_out > 0.01 {
        details(view, ui, geometry, photos, &entry);
    }

    // What was on the card, while there is nothing else to put on the stage.
    // A large photograph takes a moment to read, and until this that moment
    // was an empty rectangle growing out of the card somebody had just
    // pressed, with the word *Reading…* in the middle of it.
    //
    // `Fit::Cover` because that is how the card holds it, and this rectangle
    // starts as the card's own — the picture has to be the same picture on the
    // frame the animation begins on.
    let stood_in = view.standing_in().is_some_and(|path| {
        let path = path.to_path_buf();
        let rect = view.standing_rect(ui.picture_aspect(&path));
        ui.picture(rect, view.stage_radius(), &path, Fit::Cover, 1.0)
    });

    // Nothing else at all is drawn where the picture goes. It is not that the
    // stage is empty — it is that whatever were drawn there would be under the
    // photograph, and therefore never seen.
    if let Some(path) = view.photograph() {
        photos.touch(path);
        if photos.refused(path) {
            let line = ui.line(Text::Body);
            ui.label(
                [
                    view.stage[0],
                    view.stage[1] + view.stage[3] * 0.5 - line * 0.5,
                    view.stage[2],
                    line,
                ],
                Text::Body,
                crate::i18n::text("this-picture-cannot-be-shown"),
                Role::TextSoft,
                Align::Centre,
            );
        } else if !photos.ready(path) {
            // Said only when there is nothing to look at. The picture's own
            // thumbnail growing out of its card says the same thing better,
            // and a word over it would be a caption on the animation.
            if !stood_in {
                let line = ui.line(Text::Caption);
                ui.label(
                    [
                        view.stage[0],
                        view.stage[1] + view.stage[3] * 0.5 - line * 0.5,
                        view.stage[2],
                        line,
                    ],
                    Text::Caption,
                    crate::i18n::text("reading"),
                    Role::TextSoft,
                    Align::Centre,
                );
            }
        } else {
            placements.push((path.to_path_buf(), view.placement_now()));
        }
    }

    // The picture being left behind, still where it was, on its way out. It is
    // drawn after the one arriving so that the two cross rather than one
    // replacing the other — nothing vanishes before its transition ends.
    if let Some((path, placement)) = &view.leaving {
        let mut placement = *placement;
        placement.opacity = view.leaving_out;
        placement.within = view.stage;
        placements.push((path.clone(), placement));
    }
}

fn position(view: &View) -> String {
    let total = view.folder.photographs();
    let before = view
        .folder
        .entries
        .iter()
        .take(view.cursor)
        .filter(|entry| !entry.is_folder())
        .count();
    if total == 0 {
        return String::new();
    }
    let mut said = crate::message!("place-in-folder", "place" => before + 1, "total" => total);
    if let Zoom::Actual(scale) = view.zoom {
        said.push_str(&format!("  ·  {:.0}%", scale * 100.0));
    } else if view.fit > 0.0 {
        said.push_str(&format!("  ·  {:.0}%", view.fit * 100.0));
    }
    said
}

/// The pane of details, down the right-hand side.
///
/// A pane rather than a strip over the picture, for the reason the whole
/// application is shaped around: it is drawn by the toolkit and the photograph
/// is drawn over that, so a panel on top of one would be a panel underneath
/// it. The stage has already given up exactly this much room.
fn details(
    view: &View,
    ui: &mut Ui,
    geometry: &Geometry,
    photos: &Photos,
    entry: &crate::library::Entry,
) {
    // **Slid in from the edge rather than faded up.** The picture is drawn
    // over the frame the toolkit composed, so the stage has to stand clear of
    // this pane at every moment of the animation and not only at the end of it
    // — and a pane that arrives at the speed of a fade while the picture
    // beside it moves at the speed of a spring is a pane with a photograph on
    // top of it for a fifth of a second. The rectangle is asked for rather
    // than worked out here, so that the stage and the pane cannot disagree
    // about where it is: see `View::details_pane`.
    let rect = view.details_pane(geometry);
    ui.card(rect, Surface::Sidebar, Role::Glass, 1.0);

    let padding = ui.m(Metric::PanelPadding);
    let mut at = rect[1] + padding;
    let inner = (rect[2] - padding * 2.0).max(1.0);
    let left = rect[0] + padding;

    let say = |ui: &mut Ui, at: &mut f32, label: &str, value: &str| {
        if value.is_empty() {
            return;
        }
        let caption = ui.line(Text::Caption);
        let body = ui.line(Text::Body);
        ui.label(
            [left, *at, inner, caption],
            Text::Caption,
            label,
            Role::TextSoft,
            Align::Left,
        );
        *at += caption;
        ui.label(
            [left, *at, inner, body],
            Text::Body,
            value,
            Role::Text,
            Align::Left,
        );
        *at += body + ui.s(14.0);
    };

    let name = fit_text(ui, Text::Body, &entry.name, inner);
    say(ui, &mut at, crate::i18n::text("name"), &name);
    let size = photos
        .size(&entry.path)
        .map(|(width, height)| facts::pixels(width, height))
        .unwrap_or_default();
    say(ui, &mut at, crate::i18n::text("size"), &size);
    say(
        ui,
        &mut at,
        crate::i18n::text("on-disk"),
        &facts::size(entry.bytes),
    );
    if let Some(changed) = entry.changed {
        say(
            ui,
            &mut at,
            crate::i18n::text("written"),
            &facts::when(changed),
        );
    }
    let folder = entry
        .path
        .parent()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    let folder = cut_from_the_front(ui, &folder, inner);
    say(ui, &mut at, crate::i18n::text("folder"), &folder);
}

// ---- the head of the page ------------------------------------------------

fn head(ui: &mut Ui, geometry: &Geometry, mark: &str, title: &str, aside: &str, icons: IconStyle) {
    let line = ui.line(Text::Title);
    let glyph = ui.s(34.0);
    let middle = geometry.head * 0.55;
    ui.icon_tinted(
        [geometry.margin, middle - glyph * 0.5, glyph, glyph],
        mark,
        icons,
        Role::Accent,
        1.0,
    );

    let caption = ui.line(Text::Caption);
    let width = ui.measure(Text::Caption, aside);
    let right = geometry.window[0] - geometry.margin;
    if !aside.is_empty() {
        ui.label(
            [right - width, middle - caption * 0.5, width, caption],
            Text::Caption,
            aside,
            Role::TextSoft,
            Align::Right,
        );
    }

    let from = geometry.margin + glyph + ui.s(14.0);
    let room = (right - width - ui.s(24.0) - from).max(1.0);
    // Cut before it is drawn, or a long file name is written straight over the
    // count in the far corner. See `fit_text`.
    let title = fit_text(ui, Text::Title, title, room);
    ui.label(
        [from, middle - line * 0.5, room, line],
        Text::Title,
        &title,
        Role::Text,
        Align::Left,
    );
}

/// Shorten a path to fit, taking it off the front.
///
/// The front, because the end of a path is the part that says which folder
/// this is; the shell's own chooser cuts one the same way. A word that is
/// drawn wider than the pane it is in is not clipped by the renderer — every
/// quad is drawn before every word — so anything that might not fit has to be
/// cut before it is asked for.
fn cut_from_the_front(ui: &mut Ui, path: &str, width: f32) -> String {
    if ui.measure(Text::Body, path) <= width {
        return path.to_string();
    }
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    for skip in 1..parts.len() {
        let shorter = format!("…/{}", parts[skip..].join("/"));
        if ui.measure(Text::Body, &shorter) <= width {
            return shorter;
        }
    }
    // Even the last name is too wide: take characters off it until it is not.
    let last = parts.last().copied().unwrap_or(path);
    let mut chars: Vec<char> = last.chars().collect();
    while !chars.is_empty() {
        let shorter: String = std::iter::once('…').chain(chars.iter().copied()).collect();
        if ui.measure(Text::Body, &shorter) <= width {
            return shorter;
        }
        chars.remove(0);
    }
    String::from("…")
}
