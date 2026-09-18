# Imagonsole

**A photo viewer for [LineXinBar](https://github.com/Petexy/LineXinBar), driven
by a controller. It is shown as *Pictures*.**

`imagonsole` is the project, the package and the command; **Pictures** is what
it is called on the desktop entry, in the menu and on the window — the same
split [DistriBumpy](https://github.com/Petexy/distribumpy) has, which ships as
`distribumpy` and is shown as Software Hub.

Built on [lxb-toolkit](https://github.com/Petexy/lxb-toolkit): the shell's own
colours, glass, motion, type and marks, so a folder of photographs sits beside
the shell rather than in front of it.

![A folder](docs/folder.png)

[![Licence](https://img.shields.io/badge/licence-GPL--3.0--only-blue)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.9.0-informational)](VERSION)
[![Rust](https://img.shields.io/badge/rust-1.87%2B-orange)](Cargo.toml)

---

## What it is

```sh
imagonsole                       # the pictures folder
imagonsole ~/Holiday             # that folder
imagonsole ~/Holiday/beach.jpg   # that picture, in its folder
```

A folder is a grid the light travels across. **Only the card being looked at
is drawn as something to press** — the rest are the picture and its name.
A wall of pictures in which every one is a button is a wall with nothing picked
out. Names too long for their card are cut out of the middle, so the end that
says *which* screenshot this is survives along with the extension.

 Opening a picture fills the screen
with **the photograph itself, at its own resolution** — not a thumbnail of it —
and the pictures either side are read before they are asked for, so stepping
through a folder shows a photograph rather than a wait for one. **One picture
crosses to the next half again as fast as a panel moves** — stepping is the one
thing anybody does repeatedly here, press and look and press again, and at a
panel's speed it read as the viewer thinking about it in between.

![One picture](docs/picture.png)

## Opening one

**A photograph opens out of the card it was pressed on, and goes back into
it.** The thumbnail on that card grows until it fills the page, and the wall of
pictures steps back and fades out behind it — the same gesture a page makes
when a menu opens over it, at the size of a whole change of page. Back runs the
whole of it in reverse: the picture shrinks into the card it came from,
dissolving into the card's own thumbnail as it lands, while the wall comes
forward.

One number drives the whole of it — where the picture is, how round its corners
are, how far the wall has stepped back — so that all of them land on the same
frame.

**The card grows into its own thumbnail while the full-sized picture is read.**
A large photograph takes a moment, and the thumbnail is the picture that was
pressed, so there is something to look at from the first frame rather than an
empty rectangle and the word *Reading…*. The crop a card puts on a thumbnail
unwinds as it grows: a card fills itself with the picture and the viewer shows
the whole of it, and easing between the two is what makes the first frame the
card exactly and the last the photograph exactly.

## The controls

The whole point of the design language is that a controller, a keyboard and a
pointer are one interface rather than three. Nothing here is a controller
*mode*.

| | A pad | A keyboard, a mouse |
|---|---|---|
| move; in the viewer, **step or pan** | D-pad, left stick | arrows, `wasd`, `hjkl` |
| **zoom, by however much** | **the triggers** | the wheel |
| open, and walk the zoom stops | **A** | Enter, Space |
| back, or **close** | **B** | Escape |
| the Options menu | **Y** | F10, Menu, right-click |
| a slideshow | **Start** | `x` |
| the picture before or after | **LB** / **RB** | Shift-Tab / Tab |

and, on a keyboard alone: `+` `-` zoom a stop, `f` fit, `z` actual size, `r`
and `R` turn, `i` the details, `n` `p` the next and previous picture, `g` back
to the folder, `o` open another folder.

**Back closes it at the top of the walk.** Whatever it was opened on is that
top, and the legend says **Close** rather than Back wherever pressing it would
close.

* Opened on a **picture** — a file manager handing over a double-click —
  somebody asked to see *that photograph*. The folder behind it is there so the
  arrows have somewhere to go, not because they asked to browse it, so Back
  closes. *Back to the folder*, on the Options menu, is how they say otherwise.
* Opened on a **folder**, that folder is the top. Back walks up to it from
  anything inside it and closes at it, rather than carrying on out through the
  home directory to the root of the disk — three folders nobody asked to see,
  and four presses to leave. *Up a folder* still goes above it, and doing so
  moves the top with them.

**Two kinds of zoom, because there are two kinds of control.** A button can
only say *now*, so `A` walks a ladder of stops — fit, actual size, twice, four
times — and springs between them. A trigger and a wheel say *how much*, so they
zoom continuously and are deliberately not sprung: the smoothness is already in
the hand, and a spring between the two would only feel like lag. A wheel zooms
about the pointer, so the picture grows towards the hand rather than away from
it. A picture is never taken out past fitting, and a press afterwards goes on
from wherever the wheel actually left it.

**One rule makes the directions unambiguous.** In the viewer a direction *pans*
wherever the picture is larger than the screen, and *steps to the next picture*
where it is not. Nothing has to be switched on, and the picture itself says
which it is going to be. `A` walks the zoom stops — fit, actual size, twice,
four times — so one button reaches every useful size, and panning keeps all
four directions to itself.

A held direction pans on an **accelerating ramp**, the way the shell moves a
floating window: a rate that crosses a large photograph in reasonable time is
far too fast to land with, and one that lands well never gets across.

![Actual size](docs/zoom.png)

## If the controller does nothing

```sh
imagonsole --controllers
```

There are two completely different reasons a pad can appear to be ignored, and
from the outside they look identical: the application not reading it, or there
being no gamepad on the machine to read. That flag says which.

The second is more common than it sounds. **A controller whose driver is not in
the kernel presents no gamepad at all** — a Steam Controller run outside the
session shell that drives it appears as a mouse and a keyboard and nothing
else, so there is nothing there for this or any other program to read. Under
LineXinBar the shell is that driver and the pad is there; on a plain desktop,
with neither the shell nor Steam running, `ls /dev/input/js*` finds nothing.

`IMAGONSOLE_DEBUG_ACTIONS=1` in the environment prints every action the
application is driven by, and how far the triggers are pulled, which separates
"the control never arrived" from "it arrived and did nothing".

## What else it does

- **The right way up.** A camera writes the sensor's rows and a note saying
  which way it was held; the note is read, so a portrait is a portrait.
- **The order a person reads in.** `IMG_9` before `IMG_10`, which plain byte
  order gets exactly backwards — and a camera's own numbering is nearly always
  the order a folder is meant to be in.
- **Nine sort orders**, folders first in all of them, hidden names on request.
- **Turn, slideshow, and the details** — how many pixels, how large on disk,
  when it was written.
- **Somewhere else to look**: *Open another folder* puts the question to this
  desktop's own file chooser through the portal, and draws the toolkit's own
  where there is no portal to ask.

![The Options menu](docs/options.png)
![The details](docs/details.png)

## How it is built

Nine hundred lines of interface and about five hundred of picture, over the
toolkit. Two things are worth knowing before reading it.

**It draws its own window**, which most applications built on the toolkit do
not need to do. `Ui::picture` reads a file into one 512-pixel cell of a shared
atlas — exactly right for the card of a grid, and about a quarter of what a
photograph filling a 1080p screen needs. A viewer whose whole purpose is the
picture cannot show a soft one. So the frame is composed by `lxb-render` as
everywhere else, and the photograph is drawn over it at its own resolution, in
a pass of this application's own: its own texture, its own mip chain, trilinear
and anisotropic, in the same colour space as the page under it.

**Everything else follows from that.** The photograph is drawn *over* the
composed frame, so nothing the toolkit draws can appear on top of one. Rather
than fight it, the picture is given a **stage** — a rectangle it may occupy —
and everything else is laid out outside it. Opening the Options menu narrows
the stage and the picture steps aside; a dialog or the file chooser takes the
screen and the picture fades out. That is why the stage is animated state
rather than a rectangle worked out while drawing.

**A change of page is drawn twice over.** Both pages are on the screen while a
photograph is opening or leaving, and the toolkit draws every quad of a layer
before every word of it — so a name on a card is drawn *over* whatever the same
frame puts on top of it, however late. `Ui::recede_behind` is the door on to
both halves of the answer: it steps the wall back and fades it out, and it
takes the words out from under the picture. It is the same call a menu makes
over the page it opens on.

```text
src/main.rs      the window, the input, and --shot
src/view.rs      what is being looked at, and what every control does to it
src/draw.rs      putting it on the screen
src/photo.rs     the photograph, at its own resolution
src/library.rs   what is in a folder, and in what order
src/legend.rs    what the buttons do, drawn rather than spelled out
src/pad.rs       how far the triggers are pulled, and nothing else
src/facts.rs     how large, how many pixels, and when
```

`pad.rs` is the one place this goes past `lxb-input`, and only for the
triggers. An action is a thing that happened and a trigger is a quantity, and
there is no honest way to say "sixty percent" in a list of actions — zoom is
the one control here that genuinely wants the analogue. It maps nothing and has
no opinion about any button: `lxb_toolkit::input` stays the only thing in this
application that decides what a control *means*.

## Build

```sh
cargo build --release
cargo run --release -- ~/Pictures
```

It needs `lxb-toolkit` 0.9.0 installed — the crate sources it compiles against
live in `/usr/share/lxb-toolkit/crates`. Nothing of the toolkit is linked at
run time; cargo compiles it in.

## Verify

```sh
cargo test --release --locked
cargo clippy --all-targets --locked -- -D warnings
cargo fmt --check
```

and the one worth knowing about:

```sh
imagonsole --shot page.png ~/Pictures --width 1600 --height 900
```

`--shot` writes one settled frame to a PNG **with no display at all**, through
the same renderer and the same photograph pass the window uses. Every animation
is put where it is going first, so what it photographs is the page at rest. It
takes `--view`, `--details`, `--menu`, `--row N`, `--zoom N`, `--turn N` and
`--pan left|right|up|down`, and each of them is applied *after* a frame has
been measured — exactly as a real press is, because a direction only knows
whether it pans or steps once the picture has been measured against the stage.
Every screenshot above was taken that way.

`--after SECONDS` is the exception, and the only way to photograph an
*animation*: the page is settled first, then pressed, and the picture is taken
exactly that long afterwards. Which press waits is
`--then view|back|details|menu|next` (`--back` is the same as `--then back`);
everything else asked for happens before the loop, settled.

```sh
imagonsole --shot opening.png ~/Pictures --view --after 0.17
imagonsole --shot leaving.png ~/Pictures --view --back --after 0.17
```

## Install

```sh
./packaging/install.sh --destdir /tmp/stage
```

or a real package:

```sh
./packaging/build.sh arch      # | debian | fedora | nix
./packaging/build.sh check     # what a package would have to agree with
```

See [`packaging/README.md`](packaging/README.md).

## Languages

English and Polish, in whichever one the session speaks — on LineXinBar, the
one Settings > Language names. See [localization](docs/localization.md) for the
catalogs, how to look at a page in the other language, and how to add one.

## Licence

[GPL-3.0-only](LICENSE), matching LineXinBar and the toolkit.
