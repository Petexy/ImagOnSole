//! Imagonsole — a photo viewer for LineXinBar, driven by a controller.
//!
//!     imagonsole                       # the pictures folder
//!     imagonsole ~/Holiday             # that folder
//!     imagonsole ~/Holiday/beach.jpg   # that picture, in its folder
//!     imagonsole --shot page.png       # one settled frame, with no display
//!
//! It is an ordinary Wayland application — no shell protocol, nothing private
//! — and it runs under GNOME or Plasma as readily as under LineXinBar.
//!
//! **It draws its own window**, which most applications built on the toolkit
//! do not need to do. The reason is in `photo.rs`: the picture is drawn at its
//! own resolution in a pass of this application's own, over the frame
//! `lxb-render` composed. Everything else on the screen — the glass, the
//! cards, the light that travels between them, the menu, the chooser, the
//! marks and the words — is the toolkit answering for the material.
//!
//! Three threads: this one, which draws, and two reading photographs.

mod demo;
mod draw;
mod facts;
mod i18n;
mod legend;
mod library;
mod pad;
mod photo;
mod view;

use std::path::PathBuf;
use std::sync::Arc;

use lxb_input::Controls;
use lxb_render::{Spot, Ui, WallpaperClock};
use lxb_sound::Sounds;
use lxb_toolkit::{
    accent::Accent,
    input::{Action, Key, Wheel},
    settings::ShellTheme,
    sound::Sound,
};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::ModifiersState,
    platform::wayland::WindowAttributesExtWayland,
    window::{CursorIcon, Window, WindowId},
};

use library::Order;
use photo::Photos;
use view::{Command, Geometry, Mode, View, Zoom};

/// The stable id, which agrees with the desktop entry, the executable name and
/// `StartupWMClass`. See the toolkit's docs/application-development.md.
///
/// It is **not** what the application is called to a person. The visible name
/// is for people and may be anything; this one is for matching a launched
/// process to the window that appeared, and it has to agree in five places.
const APP_ID: &str = "imagonsole";

/// What the window is drawn into. `Bgra8UnormSrgb` is what a Wayland surface
/// wants; the picture's own pass is built for whichever this is, so that a
/// photograph and the page under it are blended in the same space.
const SURFACE: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let result = if arguments.iter().any(|one| one == "--version") {
        println!("imagonsole {}", env!("CARGO_PKG_VERSION"));
        Ok(())
    } else if arguments.iter().any(|one| one == "--help" || one == "-h") {
        println!("{HELP}");
        Ok(())
    } else if arguments.iter().any(|one| one == "--controllers") {
        controllers();
        Ok(())
    } else if let Some(at) = arguments.iter().position(|one| one == "--shot") {
        match arguments.get(at + 1) {
            Some(path) => shot(path, &arguments),
            None => Err(String::from("--shot needs a file to write")),
        }
    } else {
        window(&arguments)
    };
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

/// Where to open: the made-up folder, what was named on the command line, or
/// the user's own pictures.
///
/// `except` is the file `--shot` is going to write. It is a path on the
/// command line that is not a folder to open, and it is the only one.
fn opening(arguments: &[String], except: Option<&str>) -> Result<PathBuf, String> {
    if arguments.iter().any(|one| one == "--demo") {
        return demo::folder();
    }
    Ok(arguments
        .iter()
        .filter(|one| Some(one.as_str()) != except)
        .find(|one| !one.starts_with('-'))
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .unwrap_or_else(library::default_folder))
}

fn window(arguments: &[String]) -> Result<(), String> {
    let theme = ShellTheme::load();
    // Read once, before any thread of this process starts, because reading it
    // takes it out of the environment. A window opening in front of a
    // wallpaper already on screen draws the second that screen is showing
    // rather than starting the animation again in front of the user.
    let wallpaper =
        WallpaperClock::from_environment(theme.accent.name).unwrap_or_else(WallpaperClock::local);

    let event_loop = EventLoop::new().map_err(|err| err.to_string())?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut application = Application::new(opening(arguments, None)?, theme, wallpaper);
    event_loop
        .run_app(&mut application)
        .map_err(|err| err.to_string())
}

struct Application {
    instance: wgpu::Instance,
    window: Option<Arc<Window>>,
    surface: Option<wgpu::Surface<'static>>,
    ui: Option<Ui>,
    photos: Option<Photos>,

    view: View,
    theme: ShellTheme,
    /// The palette as the renderer wants it. Built once from the setting: the
    /// shell publishes no accent-change protocol to ordinary applications, so
    /// this is a snapshot taken at startup rather than something to follow.
    accent: Accent,
    geometry: Geometry,

    opened: std::time::Instant,
    last: std::time::Instant,
    /// The wallpaper's own clock, which is the one thing here that may have
    /// started before this process did. Everything else — transitions, key
    /// repeat, the pan ramp — runs on `opened`, because they are this window's
    /// own time and must not inherit a second from another process.
    wallpaper: WallpaperClock,

    controls: Controls,
    /// Only the triggers, and only because zoom wants an amount rather than an
    /// event. Everything else about every controller is `controls`. See
    /// `src/pad.rs`.
    pad: pad::Pad,
    sounds: Sounds,
    pointer: [f32; 2],
    wheel: Wheel,
    shift: bool,
    hand: bool,
    /// Which control the legend should picture. A key or a click says a
    /// keyboard, an action off a pad says a pad — and before either has
    /// happened, whether a pad is plugged in at all.
    pad_in_hand: bool,
    /// `IMAGONSOLE_DEBUG_ACTIONS` in the environment: every action this is
    /// driven by, on stderr. It is the only way to tell a control that is not
    /// reaching the application from one that is reaching it and doing
    /// nothing, and those two have completely different causes.
    say_actions: bool,
    said_pull: f32,
}

impl Application {
    fn new(at: PathBuf, theme: ShellTheme, wallpaper: WallpaperClock) -> Application {
        let controls = Controls::new();
        if let Some(trouble) = controls.trouble() {
            // Said once and never again: a controller is an enhancement, not a
            // startup requirement, and this is the one line somebody with a
            // dead pad will go looking for.
            eprintln!("no controller input: {trouble}");
        }
        let pad = pad::Pad::new();
        if let Some(trouble) = pad.trouble() {
            eprintln!("no trigger zoom: {trouble}");
        }
        // Said once, at startup, because "the controller does nothing" is the
        // hardest thing to diagnose from the outside and this is the one line
        // that separates its two causes. A pad with no kernel driver — a Steam
        // Controller, say, outside the session shell that drives it — leaves
        // no gamepad on the machine for anything at all to read, and nothing
        // else here would ever say so.
        if controls.pads() == 0 {
            eprintln!(
                "imagonsole: no controller found; the keyboard and mouse still work.\n\
                 imagonsole: run `imagonsole --controllers` to see what was looked at."
            );
        }
        let pad_in_hand = controls.pads() > 0;
        Application {
            instance: lxb_render::instance(),
            window: None,
            surface: None,
            ui: None,
            photos: None,
            view: View::new(&at, Order::default(), false),
            accent: Accent::new(theme.accent.name).unwrap_or_else(Accent::default_accent),
            theme,
            geometry: Geometry::of([1280.0, 800.0], |value| value, 12.0),
            opened: std::time::Instant::now(),
            last: std::time::Instant::now(),
            wallpaper,
            controls,
            pad,
            sounds: Sounds::new(),
            pointer: [0.0; 2],
            wheel: Wheel::default(),
            shift: false,
            hand: false,
            pad_in_hand,
            say_actions: std::env::var_os("IMAGONSOLE_DEBUG_ACTIONS").is_some(),
            said_pull: 0.0,
        }
    }

    fn now(&self) -> std::time::Duration {
        self.opened.elapsed()
    }

    /// Do one action and answer it.
    ///
    /// The single place a sound is played, so that a move made with a
    /// direction and the same move made with a click cannot sound different.
    fn act(&mut self, action: Action) {
        let answer = self.view.act(action);
        self.answer(answer);
    }

    fn answer(&mut self, sound: Option<Sound>) {
        if let Some(sound) = sound {
            self.sounds.play(sound);
        }
    }

    fn spot(&self) -> Spot {
        let [x, y] = self.pointer;
        self.ui
            .as_ref()
            .map(|ui| ui.at(x, y))
            .unwrap_or(Spot::Nothing)
    }

    /// A click, wherever it landed.
    fn press_at(&mut self, spot: Spot, right: bool) {
        if self.view.files.busy() {
            let answer = self.view.files.press_at(spot, right);
            self.answer(answer);
            return;
        }
        if right {
            self.act(Action::Menu);
            return;
        }
        if self.view.menu.is_open() {
            // A click on a row of the menu is a press of that row; anywhere
            // else dismisses it, which is how every panel on every desktop
            // closes.
            match spot {
                Spot::MenuRow { .. } => self.act(Action::Accept),
                _ => {
                    self.view.menu.close();
                    self.sounds.play(Sound::Back);
                }
            }
            return;
        }
        if self.view.dialog.is_open() {
            if matches!(spot, Spot::DialogButton(_)) {
                self.act(Action::Accept);
            }
            return;
        }
        // The legend is a row of controls: a click on a pair is a press of the
        // button it pictures, which is how a mouse reaches a Back that is only
        // ever drawn there.
        if let Spot::Control(id) = spot {
            let hints = draw::hints(&self.view);
            if let Some(button) = legend::pressed(id, &hints, self.pad_in_hand) {
                self.act(button.action());
                return;
            }
        }
        let answer = self.view.press_at(spot);
        if answer.is_some() {
            self.view.pressing.press();
        }
        self.answer(answer);
    }

    /// The letters this application binds for itself.
    ///
    /// Deliberately clear of `Action::of_letter`'s `wasd`, `hjkl` and `y`,
    /// which stay what they are everywhere else in this language: moving.
    fn letter(&mut self, letter: char) -> bool {
        let answer = match letter {
            '+' | '=' => self.view.zoom_step(1),
            '-' | '_' => self.view.zoom_step(-1),
            'f' | 'F' => self.view.command(Command::ZoomTo(Zoom::Fit)),
            'z' | 'Z' => self.view.command(Command::ZoomTo(Zoom::Actual(1.0))),
            'r' => self.view.command(Command::Turn(1)),
            'R' => self.view.command(Command::Turn(-1)),
            'i' | 'I' => self.view.command(Command::Info),
            'x' | 'X' => self.view.command(Command::Slideshow),
            'n' | 'N' => self.view.step_photo(1),
            'p' | 'P' => self.view.step_photo(-1),
            'g' | 'G' => self.view.command(Command::BackToGrid),
            'o' | 'O' => self.view.command(Command::OpenFolder),
            _ => return false,
        };
        self.answer(answer);
        true
    }
}

impl ApplicationHandler for Application {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title(crate::i18n::text("app-title"))
            // An application on LineXinBar is maximised and pinned to the
            // display it launched on; this size is for every other desktop.
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 800.0))
            .with_name(APP_ID, APP_ID);
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                eprintln!("no window: {err}");
                event_loop.exit();
                return;
            }
        };
        let surface = match self.instance.create_surface(window.clone()) {
            Ok(surface) => surface,
            Err(err) => {
                eprintln!("no surface: {err}");
                event_loop.exit();
                return;
            }
        };
        let size = window.inner_size();
        let ui = match pollster::block_on(Ui::new(
            &self.instance,
            Some(&surface),
            SURFACE,
            size.width,
            size.height,
        )) {
            Ok(ui) => ui,
            Err(message) => {
                eprintln!("{message}");
                event_loop.exit();
                return;
            }
        };

        configure(&surface, &ui, size.width, size.height);
        self.photos = Some(Photos::new(&ui.device, SURFACE));
        self.window = Some(window);
        self.surface = Some(surface);
        self.ui = Some(ui);
        self.opened = std::time::Instant::now();
        self.last = self.opened;
        self.wallpaper.restart();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(window) = self.window.clone() else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let (Some(surface), Some(ui)) = (self.surface.as_ref(), self.ui.as_ref()) {
                    configure(surface, ui, size.width, size.height);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer = [position.x as f32, position.y as f32];
                self.pad_in_hand = false;
                let spot = self.spot();
                if self.view.files.busy() {
                    self.view.files.point_at(spot);
                } else {
                    self.view.point_at(spot);
                }
                let hand = spot.pressable();
                if hand != self.hand {
                    self.hand = hand;
                    window.set_cursor(if hand {
                        CursorIcon::Pointer
                    } else {
                        CursorIcon::Default
                    });
                }
            }
            WindowEvent::CursorLeft { .. } => self.wheel.reset(),
            WindowEvent::MouseInput { state, button, .. } => {
                // On the press rather than the release, as every other control
                // in this language fires.
                if state == ElementState::Released {
                    return;
                }
                self.pad_in_hand = false;
                let spot = self.spot();
                self.press_at(spot, button == MouseButton::Right);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let notches = match delta {
                    MouseScrollDelta::LineDelta(_, lines) => self.wheel.notches(-lines),
                    MouseScrollDelta::PixelDelta(at) => self.wheel.distance(-at.y as f32),
                };
                self.pad_in_hand = false;
                // Where it was pointed goes with it. In the viewer that is
                // what the picture grows towards; everywhere else the view
                // turns it back into the Up and Down every list already knows.
                let answer = self.view.wheel(notches, self.pointer);
                self.answer(answer);
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.shift = modifiers.state().contains(ModifiersState::SHIFT);
            }
            // A window that has lost focus has had every control let go of.
            WindowEvent::Focused(false) => self.controls.release(),
            WindowEvent::KeyboardInput { event, .. } => {
                // The platform's own repeat is dropped: the pace a held
                // direction moves at belongs to the interface, and `lxb-input`
                // invents the same middle for an arrow key as for a D-pad.
                if event.repeat {
                    return;
                }
                let down = event.state == ElementState::Pressed;
                let key = lxb_input::key_of(&event, self.shift);
                if down {
                    self.pad_in_hand = false;
                }
                // Everything a keyboard means to the chooser, in one call.
                if down && self.view.files.busy() {
                    self.view.files.key(key, event.text.as_deref());
                    return;
                }
                let Some(key) = key else {
                    return;
                };
                if let Key::Letter(letter) = key {
                    if down && !self.letter(letter) {
                        if let Some(action) = Action::of_letter(letter) {
                            self.act(action);
                        }
                    }
                    return;
                }
                let now = self.now();
                if let Some(action) = self.controls.key(key, down, now) {
                    self.act(action);
                }
            }
            WindowEvent::RedrawRequested => self.frame(event_loop, &window),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }
}

impl Application {
    fn frame(&mut self, event_loop: &ActiveEventLoop, window: &Arc<Window>) {
        let now = std::time::Instant::now();
        // An application stopped while it is hidden must not treat the time it
        // was away as one frame; clamp it, as the toolkit's own spring does.
        let dt = (now - self.last).as_secs_f32().clamp(0.0, 0.1);
        self.last = now;

        let size = window.inner_size();
        let elapsed = self.wallpaper.elapsed_secs();
        let (Some(surface), Some(ui), Some(photos)) = (
            self.surface.as_ref(),
            self.ui.as_mut(),
            self.photos.as_mut(),
        ) else {
            return;
        };

        photos.settle(&ui.device, &ui.queue);
        ui.begin(
            size.width as f32,
            size.height as f32,
            elapsed,
            &self.accent,
            self.theme.wallpaper,
            self.theme.icons,
        );

        self.geometry = Geometry::of(
            [size.width as f32, size.height as f32],
            |value| ui.s(value),
            ui.m(lxb_toolkit::metrics::Metric::Gap),
        );
        // Measure, act, move, draw — in that order, so that a direction is
        // answered against the picture as it is this frame rather than as it
        // was before the last animation settled.
        self.view.measure(&self.geometry, photos);

        // Everything the controllers have to say, and the repeats of whichever
        // key is being held — one list, in which nothing says which of the two
        // it came from. After the measuring, so that a direction is answered
        // against the picture as it is this frame: whether it pans or steps to
        // the next picture depends on a size that was only just worked out.
        let actions = self.controls.poll(self.opened.elapsed());
        if !actions.is_empty() {
            self.pad_in_hand = self.controls.pads() > 0;
        }
        // And the one thing a list of actions cannot carry: how far.
        self.pad.settle();
        let pull = self.pad.pull();
        if pull != 0.0 {
            self.pad_in_hand = true;
        }
        self.view.set_pull(pull);
        if self.say_actions && (pull - self.said_pull).abs() > 0.05 {
            self.said_pull = pull;
            eprintln!(
                "imagonsole: triggers {pull:+.2} -> {:.0}%",
                self.view.effective_scale() * 100.0
            );
        }
        for action in actions {
            if self.say_actions {
                eprintln!("imagonsole: {action:?}");
            }
            // The fields are reached one at a time rather than through
            // `answer`, because `ui` is borrowed for the whole of this frame
            // and a method taking all of `self` would want it back.
            if let Some(sound) = self.view.act(action) {
                self.sounds.play(sound);
            }
        }

        // A folder chosen through the chooser, whether it answered here or in
        // the desktop's own panel.
        if let Some(chosen) = self.view.files.answered() {
            if let Some(path) = chosen.into_iter().next() {
                self.view.browse_from(&path);
            }
        }
        self.view.files.hand(self.pad_in_hand);
        if self.view.quit {
            event_loop.exit();
            return;
        }

        self.view.advance(dt, &self.geometry);
        photos.want(&self.view.wanted());

        // A menu asked for this frame is raised here, where there is
        // something that can measure a word: its title has to be cut to the
        // panel before it is handed over. See `View::settle_menu`.
        self.view
            .settle_menu(&mut |text, string| ui.measure(text, string));

        let placements = draw::draw(
            &mut self.view,
            ui,
            &self.geometry,
            photos,
            self.pad_in_hand,
            self.theme.icons,
        );

        use wgpu::CurrentSurfaceTexture as Acquired;
        match surface.get_current_texture() {
            Acquired::Success(frame) | Acquired::Suboptimal(frame) => {
                let view = frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                if let Err(message) = ui.end(&view) {
                    eprintln!("{message}");
                }
                // And then the photograph, over the frame the toolkit composed.
                let mut encoder =
                    ui.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("photograph"),
                        });
                photos.draw(
                    &ui.device,
                    &ui.queue,
                    &mut encoder,
                    &view,
                    [size.width as f32, size.height as f32],
                    &placements,
                );
                ui.queue.submit(Some(encoder.finish()));
                ui.queue.present(frame);
            }
            Acquired::Outdated | Acquired::Lost => configure(surface, ui, size.width, size.height),
            // Occluded, timed out, or refused: skip the frame rather than draw
            // one nobody will see.
            _ => {}
        }
    }
}

fn configure(surface: &wgpu::Surface<'_>, ui: &Ui, width: u32, height: u32) {
    surface.configure(
        &ui.device,
        &wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: SURFACE,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        },
    );
}

/// One settled frame, to a PNG, with no display at all.
///
/// The same page functions, the same renderer and the same photograph pass as
/// the window — which is what makes it worth looking at, and is how an
/// interface in this language is checked without a screen.
fn shot(path: &str, arguments: &[String]) -> Result<(), String> {
    let named = |flag: &str| -> Option<String> {
        arguments
            .iter()
            .position(|one| one == flag)
            .and_then(|at| arguments.get(at + 1))
            .cloned()
    };
    let number = |flag: &str, fallback: u32| -> u32 {
        named(flag)
            .and_then(|value| value.parse().ok())
            .unwrap_or(fallback)
    };
    let width = number("--width", 1600);
    let height = number("--height", 900);

    let theme = ShellTheme::load();
    let accent = Accent::new(theme.accent.name).unwrap_or_else(Accent::default_accent);
    // An sRGB target rather than the toolkit's own float one, so that what
    // comes back is already the bytes a PNG wants.
    const SHOT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
    let instance = lxb_render::instance();
    let mut ui = pollster::block_on(Ui::new(&instance, None, SHOT, width, height))?;
    let mut photos = Photos::new(&ui.device, SHOT);

    let at = opening(arguments, Some(path))?;
    let mut view = View::new(&at, Order::default(), false);
    // A picture of a page in motion rather than at rest: settle everything
    // first, then press, then count exactly the frames that were asked for.
    // Nothing else can photograph an animation — the loop below is built to
    // wait until nothing is moving. With `--after` the *last* press waits for
    // the middle of the loop, where the page it acts on is really there: a
    // thumbnail that has not arrived is a card with nothing in it to grow out
    // of, and a picture that is not up cannot shrink back into one.
    let after = named("--after").and_then(|value| value.parse::<f32>().ok());
    // Which press waits for the middle of the loop. Anything else asked for
    // happens before it, settled, so that what is photographed is one
    // animation and not two overlapping.
    let then = named("--then").unwrap_or_else(|| {
        String::from(if arguments.iter().any(|one| one == "--back") {
            "back"
        } else {
            "view"
        })
    });
    if after.is_none() || then != "view" {
        press_the_page(&mut view, arguments);
    }
    let mut pressed = after.is_none().then_some(0);
    if arguments.iter().any(|one| one == "--details") {
        let _ = view.command(Command::Info);
    }
    if let Some(row) = named("--row").and_then(|value| value.parse::<usize>().ok()) {
        view.cursor = row.min(view.folder.entries.len().saturating_sub(1));
    }
    // Everything asked for on the command line is done to the view *after* a
    // frame has been measured, exactly as a real press is — a direction only
    // knows whether it pans or steps to the next picture once the picture has
    // been measured against the stage. Applying them here, before the first
    // frame, is how `--pan` used to walk the folder instead.
    let zoom = named("--zoom").and_then(|value| value.parse::<isize>().ok());
    let turn = named("--turn").and_then(|value| value.parse::<i32>().ok());
    let pan = named("--pan").map(|which| match which.as_str() {
        "left" => Action::Left,
        "right" => Action::Right,
        "up" => Action::Up,
        _ => Action::Down,
    });
    let mut menu_wanted =
        arguments.iter().any(|one| one == "--menu") && named("--then").as_deref() != Some("menu");
    let mut asked = false;

    let texture = ui.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shot"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: SHOT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target = texture.create_view(&wgpu::TextureViewDescriptor::default());

    // Drawn until the pictures have been read and every animation has settled,
    // so what is photographed is the page at rest rather than on its way
    // there. Two seconds of frames is far longer than any of it takes; the
    // loop leaves as soon as nothing is moving.
    let mut placements = Vec::new();
    for frame in 0..240 {
        let dt = 1.0 / 60.0;
        photos.settle(&ui.device, &ui.queue);
        ui.begin(
            width as f32,
            height as f32,
            6.0,
            &accent,
            theme.wallpaper,
            theme.icons,
        );
        let geometry = Geometry::of(
            [width as f32, height as f32],
            |value| ui.s(value),
            ui.m(lxb_toolkit::metrics::Metric::Gap),
        );
        view.measure(&geometry, &photos);
        // Once the picture has been read and measured, not before.
        if !asked && view.wanted().iter().all(|path| photos.ready(path)) {
            asked = true;
            if let Some(stops) = zoom {
                for _ in 0..stops.abs() {
                    let _ = view.zoom_step(stops.signum());
                }
            }
            if let Some(quarters) = turn {
                let _ = view.command(Command::Turn(quarters));
            }
            if menu_wanted {
                view.open_menu();
                menu_wanted = false;
            }
        }
        if let Some(pan) = pan {
            // Held, rather than pressed once: a pan is a ramp, and what is
            // worth photographing is where a held direction really reaches.
            //
            // Only once the picture really does hang over the edge *along
            // this direction's own axis*. A direction with nothing to pan to
            // steps to the next picture instead — that is the viewer's rule,
            // not an accident — so a guard that asked whether either axis
            // overflowed walked the folder rather than panning.
            let axis = usize::from(matches!(pan, Action::Up | Action::Down));
            if asked && view.overflow[axis] > 0.5 {
                let _ = view.act(pan);
            }
        }
        view.advance(dt, &geometry);
        photos.want(&view.wanted());
        view.settle_menu(&mut |text, string| ui.measure(text, string));
        placements = draw::draw(
            &mut view,
            &mut ui,
            &geometry,
            &mut photos,
            true,
            theme.icons,
        );

        // `Ui::end` takes the scene as it draws it, so it is called exactly
        // once an iteration and the last one is the frame that is kept.
        // Calling it a second time after the loop drew an empty scene over
        // the top — which is to say, black.
        ui.end(&target)?;

        let settled = asked
            && view.crossing().is_none()
            && view
                .wanted()
                .iter()
                .all(|path| photos.ready(path) || photos.refused(path));
        let held = if pan.is_some() { 120 } else { 60 };
        if let Some(after) = after {
            match pressed {
                None if settled && frame > 30 => {
                    match then.as_str() {
                        "back" => {
                            let _ = view.act(Action::Back);
                        }
                        "details" => {
                            let _ = view.command(Command::Info);
                        }
                        "menu" => view.open_menu(),
                        "next" => {
                            let _ = view.act(Action::Next);
                        }
                        _ => press_the_page(&mut view, arguments),
                    }
                    pressed = Some(frame);
                }
                Some(at) if frame >= at + (after * 60.0).round().max(0.0) as usize => break,
                _ => {}
            }
            continue;
        }
        if frame > held && settled {
            break;
        }
    }

    let mut encoder = ui
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("shot"),
        });
    photos.draw(
        &ui.device,
        &ui.queue,
        &mut encoder,
        &target,
        [width as f32, height as f32],
        &placements,
    );
    ui.queue.submit(Some(encoder.finish()));

    let pixels = read_back(&ui.device, &ui.queue, &texture, width, height)?;
    write_png(path, &pixels, width, height)
}

/// `--view`: press the picture the light is on, which is what somebody looking
/// at a folder would do.
fn press_the_page(view: &mut View, arguments: &[String]) {
    if !arguments.iter().any(|one| one == "--view") || view.mode != Mode::Grid {
        return;
    }
    let Some(first) = view
        .folder
        .entries
        .iter()
        .position(|entry| !entry.is_folder())
    else {
        return;
    };
    view.cursor = first;
    let _ = view.act(Action::Accept);
}

/// Copy the drawn texture back and hand over its bytes.
fn read_back(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, String> {
    // A copy out of a texture is written in rows padded to 256 bytes.
    let row = (width * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(row) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("readback"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));

    let slice = buffer.slice(..);
    let (send, receive) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = send.send(result);
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .map_err(|err| err.to_string())?;
    receive
        .recv()
        .map_err(|err| err.to_string())?
        .map_err(|err| err.to_string())?;

    let mapped = slice.get_mapped_range().map_err(|err| err.to_string())?;
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let from = (y * row) as usize;
        pixels.extend_from_slice(&mapped[from..from + (width * 4) as usize]);
    }
    drop(mapped);
    buffer.unmap();
    Ok(pixels)
}

fn write_png(path: &str, pixels: &[u8], width: u32, height: u32) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|err| err.to_string())?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .map_err(|err| err.to_string())?
        .write_image_data(pixels)
        .map_err(|err| err.to_string())
}

/// What this machine offers to be driven with, and what it does not.
///
/// Worth a flag of its own because a controller that does nothing has two
/// completely different causes — the application ignoring it, or there being
/// no gamepad on the machine to ignore — and from the outside they look
/// identical. A pad whose driver lives somewhere else is the ordinary case
/// here rather than a fault: a Steam Controller run outside the session shell
/// that drives it presents a mouse and a keyboard and no gamepad at all, so
/// there is nothing for this or any other program to read.
fn controllers() {
    let controls = Controls::new();
    let pad = pad::Pad::new();
    if let Some(trouble) = controls.trouble() {
        println!("No controller support at all: {trouble}");
        return;
    }

    let found = pad.found();
    if found.is_empty() {
        println!("No controllers found.");
        println!();
        println!("Nothing on this machine is presenting a gamepad. That is not");
        println!("always a fault: a controller whose driver is not in the kernel");
        println!("— a Steam Controller outside the session shell that drives it,");
        println!("for one — appears as a mouse and a keyboard and no gamepad, so");
        println!("there is nothing here for any program to read.");
        println!();
        println!("Look for one with:  ls /dev/input/js*");
        return;
    }

    println!(
        "{} controller{} found:",
        found.len(),
        if found.len() == 1 { "" } else { "s" }
    );
    for one in &found {
        println!();
        println!("  {}", one.name);
        println!(
            "    buttons   {}",
            if one.mapped {
                "named by this desktop's own mapping"
            } else {
                "guessed from the driver — the face buttons may be round the wrong way"
            }
        );
        println!(
            "    triggers  {}",
            if one.triggers {
                "yes — they zoom"
            } else {
                "none reported; zoom with A, or + and -"
            }
        );
    }
}

const HELP: &str = "\
usage: imagonsole [FILE-OR-FOLDER]
       imagonsole --demo
       imagonsole --shot FILE [FOLDER] [--width N] [--height N]
                  [--view] [--details] [--menu] [--row N]
                  [--zoom N] [--turn N] [--pan left|right|up|down]

With no arguments it opens the pictures folder. Given a folder it opens that;
given a picture it shows that picture, with its own folder behind it.

A pad             A keyboard
-----             ----------
D-pad / stick     arrows, wasd, hjkl   move; in the viewer, step or pan
A                 Enter, Space         open, and walk the zoom stops
B                 Escape               back
Y                 F10, Menu, right-click   the Options menu
Start             x                    a slideshow
LB / RB           Shift-Tab / Tab      the picture before or after
triggers          the wheel            zoom, by however much

                  + and -              zoom by one stop
                  f / z                fit / actual size
                  r / R                turn right / left
                  i                    the details
                  n / p                the picture after / before
                  g                    back to the folder
                  o                    open another folder

In the viewer a direction pans wherever the picture is larger than the screen,
and steps to the next picture where it is not.

--demo opens a made-up folder, drawn into this application's own cache.
Nothing of yours is read and nothing of yours is written. It is what the
pictures in the README were taken against.

--shot writes one settled frame to a PNG with no display at all, through the
same renderer and the same photograph pass the window uses.

--after SECONDS is the one way to photograph an animation: the page is settled
first, then pressed, and the picture taken exactly that long afterwards. Which
press waits is --then view|back|details|menu|next (--back is the same as
--then back); everything else asked for happens before the loop, settled.

--demo         a made-up folder; nothing of yours is touched
--controllers  list what this machine can be driven with
--version      print the version
--help         print this message";
