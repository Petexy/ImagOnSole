//! The photograph itself, at its own resolution.
//!
//! **Why this file exists.** Everything else on the screen is drawn by
//! `lxb-render`, which is the point of building on the toolkit at all. But
//! `Ui::picture` reads a file into one 512-pixel cell of a shared atlas —
//! exactly the right size for the card of a grid, and about a quarter of what
//! a photograph filling a 1080p screen needs. A viewer whose whole purpose is
//! the picture cannot show a soft one, so the picture is drawn here instead:
//! its own texture, its own mip chain, its own pass over the frame the toolkit
//! has already composed.
//!
//! **What that costs, and what it does not.** The pass runs after `Ui::end`,
//! so the photograph is over everything the toolkit drew. That is the one
//! rule the rest of the application is written around: anything that has to
//! appear *over* a photograph — a menu, a dialog, the chooser — cannot simply
//! be drawn on top of it, so the stage the picture occupies gets out of the
//! way instead. See `view.rs`. It buys the only thing that matters here: what
//! is on the screen is the file, not a thumbnail of it.
//!
//! Nothing in here knows about folders, selection or input. It is asked for a
//! path, it answers whether it has one yet, and it draws the ones it has.

use image::ImageDecoder;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};

/// How large a photograph is allowed to become on the GPU.
///
/// Nearly every photograph a camera writes is inside this, so nearly every
/// photograph is held whole and a hundred percent really is a hundred percent.
/// What it protects against is the panorama and the scan: a 30,000-pixel image
/// is not a reason to ask for a texture no device will give.
const LONGEST_SIDE: u32 = 8192;

/// Roughly how much texture memory the cache may hold before it starts
/// letting go, in bytes.
///
/// Counted from the pixels rather than measured, because what a driver really
/// allocated is not something to ask on a frame. A photograph either side of
/// the one being looked at is worth keeping — that is what makes the next one
/// appear rather than arrive — and much more than that is only holding on to
/// pictures nobody is walking back to.
const BUDGET: usize = 768 * 1024 * 1024;

/// One photograph, decoded, with the smaller copies of itself that keep it
/// from sparkling when it is shown at less than its own size.
struct Decoded {
    width: u32,
    height: u32,
    /// Level nought first, each half the size of the one before it.
    levels: Vec<Vec<u8>>,
}

impl Decoded {
    fn bytes(&self) -> usize {
        self.levels.iter().map(Vec::len).sum()
    }
}

struct Request {
    path: PathBuf,
    longest: u32,
}

struct Answer {
    path: PathBuf,
    picture: Option<Decoded>,
}

/// What the cache knows about one path.
enum Held {
    Waiting,
    Ready(Resident),
    /// Read and refused. Kept, so the same broken file is not decoded again on
    /// every frame it is looked at.
    Refused,
}

struct Resident {
    view: wgpu::TextureView,
    bind: wgpu::BindGroup,
    width: u32,
    height: u32,
    bytes: usize,
    /// Which frame this was last drawn or asked for, for letting go of the
    /// least recently wanted first.
    used: u64,
}

/// Where one photograph goes this frame.
///
/// A rectangle, a turn about its own centre, and how much of it is there.
/// The rectangle is in the same pixels everything else on the page is laid out
/// in — the window's, top-left origin — because a viewer that had to convert
/// would convert one of the two wrongly.
#[derive(Debug, Clone, Copy)]
pub struct Placement {
    pub centre: [f32; 2],
    /// Half the drawn width and height, *before* the turn.
    pub half: [f32; 2],
    /// Clockwise, in radians, about the centre.
    pub turn: f32,
    pub opacity: f32,
    /// How much light is left in it. One, except while something is being
    /// opened over it — a menu steps the whole page back and dims it, and a
    /// photograph that stayed bright while the page behind it dimmed would be
    /// the one surface that had not heard.
    pub dim: f32,
    /// How round the picture's own corners are.
    ///
    /// Nought for a photograph on a page. One opening out of a card carries
    /// the card's rounding and loses it on the way — see
    /// [`crate::view::View::stage_radius`].
    pub radius: f32,
    /// Nothing is drawn outside this. It is what keeps a picture inside the
    /// stage it was given rather than over the legend under it.
    pub within: [f32; 4],
}

pub struct Photos {
    requests: mpsc::Sender<Request>,
    answers: mpsc::Receiver<Answer>,
    held: HashMap<PathBuf, Held>,
    wanted: Vec<PathBuf>,
    frame: u64,

    longest: u32,
    pipeline: wgpu::RenderPipeline,
    picture_layout: wgpu::BindGroupLayout,
    placing_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// One uniform buffer and bind group per photograph drawn in a frame,
    /// grown as needed and then reused. There are never many: the one being
    /// looked at, and the one it is crossing to.
    slots: Vec<Slot>,
}

struct Slot {
    buffer: wgpu::Buffer,
    bind: wgpu::BindGroup,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Placed {
    centre: [f32; 2],
    half: [f32; 2],
    turn: [f32; 2],
    screen: [f32; 2],
    opacity: f32,
    /// How round its corners are, how much light is left in it, and one word
    /// of nothing — a uniform buffer counts in sixteens.
    radius: f32,
    dim: f32,
    padding: [f32; 1],
}

impl Photos {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Photos {
        // Never zero — a device reporting no texture dimension at all would
        // otherwise ask the readers for a picture of nothing.
        let longest = device
            .limits()
            .max_texture_dimension_2d
            .clamp(1, LONGEST_SIDE);

        let placing_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("where a photograph goes"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let picture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("a photograph"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("photograph"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("photograph"),
            bind_group_layouts: &[Some(&placing_layout), Some(&picture_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("photograph"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // Trilinear, and as many samples across the slope as the device will
        // give. A photograph shown smaller than itself is the ordinary case
        // here — that is what fitting one to a screen is — and it is exactly
        // the case a single bilinear tap gets wrong.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("photograph"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });

        let (requests, answers) = readers();
        Photos {
            requests,
            answers,
            held: HashMap::new(),
            wanted: Vec::new(),
            frame: 0,
            longest,
            pipeline,
            picture_layout,
            placing_layout,
            sampler,
            slots: Vec::new(),
        }
    }

    /// The photographs worth having, most wanted first.
    ///
    /// Called every frame with the one being looked at and its neighbours.
    /// Anything already held stays held; anything new is asked for; and what
    /// is over the budget is let go of, least recently wanted first — never
    /// the ones named here, however tight it is.
    pub fn want(&mut self, paths: &[PathBuf]) {
        self.wanted.clear();
        self.wanted.extend_from_slice(paths);
        for path in paths {
            match self.held.get_mut(path) {
                Some(Held::Ready(resident)) => resident.used = self.frame,
                Some(_) => {}
                None => {
                    self.held.insert(path.clone(), Held::Waiting);
                    let _ = self.requests.send(Request {
                        path: path.clone(),
                        longest: self.longest,
                    });
                }
            }
        }
        self.let_go();
    }

    fn let_go(&mut self) {
        let mut held: usize = self
            .held
            .values()
            .map(|entry| match entry {
                Held::Ready(resident) => resident.bytes,
                _ => 0,
            })
            .sum();
        if held <= BUDGET {
            return;
        }
        let mut oldest: Vec<(PathBuf, u64)> = self
            .held
            .iter()
            .filter_map(|(path, entry)| match entry {
                Held::Ready(resident) if !self.wanted.contains(path) => {
                    Some((path.clone(), resident.used))
                }
                _ => None,
            })
            .collect();
        oldest.sort_by_key(|(_, used)| *used);
        for (path, _) in oldest {
            if held <= BUDGET {
                break;
            }
            if let Some(Held::Ready(resident)) = self.held.remove(&path) {
                held -= resident.bytes;
            }
        }
    }

    /// Take whatever the readers have finished and put it on the GPU.
    ///
    /// Once a frame, before anything is drawn. A photograph nobody wants any
    /// more by the time it arrives is dropped rather than uploaded.
    pub fn settle(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.frame = self.frame.wrapping_add(1);
        while let Ok(answer) = self.answers.try_recv() {
            if !matches!(self.held.get(&answer.path), Some(Held::Waiting)) {
                continue;
            }
            let Some(picture) = answer.picture else {
                self.held.insert(answer.path, Held::Refused);
                continue;
            };
            let resident = self.upload(device, queue, &picture);
            self.held.insert(answer.path, Held::Ready(resident));
        }
    }

    fn upload(&self, device: &wgpu::Device, queue: &wgpu::Queue, picture: &Decoded) -> Resident {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("photograph"),
            size: wgpu::Extent3d {
                width: picture.width,
                height: picture.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: picture.levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Written as sRGB so the hardware converts on the way in and the
            // sRGB target converts back on the way out. A photograph blended
            // in the wrong space is a photograph with the wrong edges.
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, pixels) in picture.levels.iter().enumerate() {
            let width = (picture.width >> level).max(1);
            let height = (picture.height >> level).max(1);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 4),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("photograph"),
            layout: &self.picture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        Resident {
            view,
            bind,
            width: picture.width,
            height: picture.height,
            bytes: picture.bytes(),
            used: self.frame,
        }
    }

    /// How large this photograph is, once it is here.
    ///
    /// `None` while it is still being read, which is the difference between a
    /// picture that has no size yet and one that has none at all.
    pub fn size(&self, path: &Path) -> Option<(u32, u32)> {
        match self.held.get(path) {
            Some(Held::Ready(resident)) => Some((resident.width, resident.height)),
            _ => None,
        }
    }

    pub fn refused(&self, path: &Path) -> bool {
        matches!(self.held.get(path), Some(Held::Refused))
    }

    pub fn ready(&self, path: &Path) -> bool {
        matches!(self.held.get(path), Some(Held::Ready(_)))
    }

    /// Draw the photographs, over the frame the toolkit has already composed.
    ///
    /// One pass, loading rather than clearing — everything under it is the
    /// wallpaper, the panes and the words, and they are all still wanted.
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        into: &wgpu::TextureView,
        screen: [f32; 2],
        placements: &[(PathBuf, Placement)],
    ) {
        let showing: Vec<(usize, &PathBuf, &Placement)> = placements
            .iter()
            .filter(|(path, placement)| placement.opacity > 0.001 && self.ready(path))
            .enumerate()
            .map(|(index, (path, placement))| (index, path, placement))
            .collect();
        if showing.is_empty() {
            return;
        }

        while self.slots.len() < showing.len() {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("where a photograph goes"),
                size: std::mem::size_of::<Placed>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("where a photograph goes"),
                layout: &self.placing_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            self.slots.push(Slot { buffer, bind });
        }

        for (slot, _, placement) in &showing {
            queue.write_buffer(
                &self.slots[*slot].buffer,
                0,
                bytemuck::bytes_of(&Placed {
                    centre: placement.centre,
                    half: placement.half,
                    turn: [placement.turn.cos(), placement.turn.sin()],
                    screen,
                    opacity: placement.opacity,
                    radius: placement.radius.max(0.0),
                    dim: placement.dim.clamp(0.0, 1.0),
                    padding: [0.0; 1],
                }),
            );
        }

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("photograph"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: into,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        for (slot, path, placement) in showing {
            let Some(Held::Ready(resident)) = self.held.get(path) else {
                continue;
            };
            let Some([x, y, width, height]) = whole_pixels(placement.within, screen) else {
                continue;
            };
            pass.set_scissor_rect(x, y, width, height);
            pass.set_bind_group(0, &self.slots[slot].bind, &[]);
            pass.set_bind_group(1, &resident.bind, &[]);
            pass.draw(0..6, 0..1);
            let _ = &resident.view;
        }
    }

    /// Mark a photograph as wanted without asking for it.
    ///
    /// What the frame loop calls for the one it is drawing, so that a picture
    /// being looked at is never the least recently wanted thing in the cache.
    pub fn touch(&mut self, path: &Path) {
        if let Some(Held::Ready(resident)) = self.held.get_mut(path) {
            resident.used = self.frame;
        }
    }
}

/// A scissor rectangle has to be whole pixels inside the target, and a
/// rectangle that came out empty is not a rectangle to draw nothing with —
/// it is a draw to skip.
fn whole_pixels([x, y, width, height]: [f32; 4], screen: [f32; 2]) -> Option<[u32; 4]> {
    let left = x.floor().max(0.0);
    let top = y.floor().max(0.0);
    let right = (x + width).ceil().min(screen[0]);
    let bottom = (y + height).ceil().min(screen[1]);
    if right <= left || bottom <= top {
        return None;
    }
    Some([
        left as u32,
        top as u32,
        (right - left) as u32,
        (bottom - top) as u32,
    ])
}

/// Two threads reading files.
///
/// Two rather than one because the picture being looked at and the one about
/// to be are both wanted at once, and rather than many because a photograph is
/// mostly memory bandwidth and eight of them at once is slower than two.
fn readers() -> (mpsc::Sender<Request>, mpsc::Receiver<Answer>) {
    let (send_request, take_request) = mpsc::channel::<Request>();
    let (send_answer, take_answer) = mpsc::channel::<Answer>();
    let queue = Arc::new(Mutex::new(take_request));
    for number in 0..2 {
        let queue = Arc::clone(&queue);
        let answers = send_answer.clone();
        let _ = std::thread::Builder::new()
            .name(format!("imagonsole-read-{number}"))
            .spawn(move || loop {
                let request = {
                    let Ok(queue) = queue.lock() else {
                        break;
                    };
                    queue.recv()
                };
                let Ok(request) = request else {
                    break;
                };
                let picture = read(&request.path, request.longest);
                if answers
                    .send(Answer {
                        path: request.path,
                        picture,
                    })
                    .is_err()
                {
                    break;
                }
            });
    }
    (send_request, take_answer)
}

/// Read one photograph, the right way up, no larger than the device will hold.
fn read(path: &Path, longest: u32) -> Option<Decoded> {
    let file = std::io::BufReader::new(std::fs::File::open(path).ok()?);
    let mut reader = image::ImageReader::new(file).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    // A photograph is allowed to be large. This is the ceiling that keeps a
    // corrupt header from asking for the machine.
    limits.max_alloc = Some(2 * 1024 * 1024 * 1024);
    reader.limits(limits);

    let mut decoder = reader.into_decoder().ok()?;
    // Cameras write the sensor's rows and a note saying which way up the
    // camera was. Ignoring the note is how a viewer shows a portrait on its
    // side, which is the single most obvious thing one can get wrong.
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut picture = image::DynamicImage::from_decoder(decoder).ok()?;
    picture.apply_orientation(orientation);

    if picture.width() > longest || picture.height() > longest {
        picture = picture.resize(longest, longest, image::imageops::FilterType::Lanczos3);
    }
    let base = picture.to_rgba8();
    let (width, height) = (base.width(), base.height());
    if width == 0 || height == 0 {
        return None;
    }

    let mut levels = vec![base.into_raw()];
    let mut level = 0;
    while (width >> level).max(1) > 1 || (height >> level).max(1) > 1 {
        let from = ((width >> level).max(1), (height >> level).max(1));
        level += 1;
        let to = ((width >> level).max(1), (height >> level).max(1));
        let smaller = halve(&levels[levels.len() - 1], from, to);
        levels.push(smaller);
    }
    Some(Decoded {
        width,
        height,
        levels,
    })
}

/// One mip level from the one above it, averaging the pixels that fall into
/// each new one.
///
/// A box filter over exactly the covering rectangle, which handles the odd
/// dimensions correctly — halving a 5-pixel row gives two pixels, and the
/// second of them is three source pixels wide rather than two.
fn halve(source: &[u8], (width, height): (u32, u32), (to_width, to_height): (u32, u32)) -> Vec<u8> {
    let mut out = vec![0u8; (to_width as usize) * (to_height as usize) * 4];
    for y in 0..to_height {
        let top = y * height / to_height;
        let bottom = (((y + 1) * height).div_ceil(to_height))
            .min(height)
            .max(top + 1);
        for x in 0..to_width {
            let left = x * width / to_width;
            let right = (((x + 1) * width).div_ceil(to_width))
                .min(width)
                .max(left + 1);
            let mut total = [0u32; 4];
            let mut count = 0u32;
            for row in top..bottom {
                for column in left..right {
                    let at = ((row as usize) * (width as usize) + column as usize) * 4;
                    for (channel, sum) in total.iter_mut().enumerate() {
                        *sum += u32::from(source[at + channel]);
                    }
                    count += 1;
                }
            }
            let at = ((y as usize) * (to_width as usize) + x as usize) * 4;
            for (channel, sum) in total.iter().enumerate() {
                out[at + channel] = (sum / count.max(1)) as u8;
            }
        }
    }
    out
}

const SHADER: &str = r#"
struct Placed {
    centre: vec2<f32>,
    half: vec2<f32>,
    turn: vec2<f32>,
    screen: vec2<f32>,
    opacity: f32,
    // How round its own corners are, and how much light is left in it.
    radius: f32,
    dim: f32,
    pad0: f32,
};

/// The distance from a rounded rectangle, negative inside it.
///
/// The same field the toolkit's own shader draws its panels with, so a picture
/// landing on a card lands on the card's own curve.
fn rounded(local: vec2<f32>, half: vec2<f32>, radius: f32) -> f32 {
    let corner = min(radius, min(half.x, half.y));
    let q = abs(local) - half + vec2<f32>(corner, corner);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0, 0.0))) - corner;
}

@group(0) @binding(0) var<uniform> placed: Placed;
@group(1) @binding(0) var picture: texture_2d<f32>;
@group(1) @binding(1) var sampling: sampler;

struct Drawn {
    @builtin(position) at: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) index: u32) -> Drawn {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
    );
    let corner = corners[index];
    let local = corner * placed.half;
    // Clockwise on the screen, whose y grows downwards.
    let turned = vec2<f32>(
        local.x * placed.turn.x - local.y * placed.turn.y,
        local.x * placed.turn.y + local.y * placed.turn.x,
    );
    let pixel = placed.centre + turned;

    var out: Drawn;
    out.at = vec4<f32>(
        pixel.x / placed.screen.x * 2.0 - 1.0,
        1.0 - pixel.y / placed.screen.y * 2.0,
        0.0,
        1.0,
    );
    out.uv = corner * vec2<f32>(0.5, 0.5) + vec2<f32>(0.5, 0.5);
    return out;
}

@fragment
fn fs(drawn: Drawn) -> @location(0) vec4<f32> {
    let colour = textureSample(picture, sampling, drawn.uv);
    // In the picture's own frame rather than the screen's, so a turned picture
    // is rounded at its own corners. Feathered over the same three quarters of
    // a pixel the toolkit feathers its own edges over.
    let local = (drawn.uv - vec2<f32>(0.5, 0.5)) * 2.0 * placed.half;
    let edge = 1.0 - smoothstep(-0.75, 0.75, rounded(local, placed.half, placed.radius));
    // Dimmed rather than faded: a picture faded toward the page behind it
    // would show the page through itself.
    //
    // Not premultiplied: the pass blends with src-alpha, so only the alpha
    // carries how much of the picture is there.
    return vec4<f32>(colour.rgb * placed.dim, colour.a * placed.opacity * edge);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halving_averages_and_keeps_the_odd_pixel() {
        // Two by one, black and white, down to one pixel: the average.
        let source = vec![0, 0, 0, 255, 255, 255, 255, 255];
        let smaller = halve(&source, (2, 1), (1, 1));
        assert_eq!(smaller, vec![127, 127, 127, 255]);
    }

    #[test]
    fn halving_an_odd_row_covers_every_source_pixel() {
        // Three across to one: all three are in it.
        let source = vec![
            0, 0, 0, 255, //
            30, 30, 30, 255, //
            60, 60, 60, 255,
        ];
        let smaller = halve(&source, (3, 1), (1, 1));
        assert_eq!(smaller[0], 30);
    }

    #[test]
    fn a_scissor_outside_the_screen_is_no_draw_at_all() {
        assert!(whole_pixels([-40.0, 0.0, 20.0, 20.0], [800.0, 600.0]).is_none());
        assert!(whole_pixels([0.0, 0.0, 0.0, 20.0], [800.0, 600.0]).is_none());
        assert_eq!(
            whole_pixels([10.4, 10.6, 100.0, 100.0], [800.0, 600.0]),
            Some([10, 10, 101, 101])
        );
    }

    #[test]
    fn a_scissor_is_cut_to_the_screen() {
        let cut = whole_pixels([700.0, 500.0, 400.0, 400.0], [800.0, 600.0]).unwrap();
        assert_eq!(cut, [700, 500, 100, 100]);
    }

    #[test]
    fn the_uniform_is_the_size_the_shader_says() {
        // vec2 x 4, then four floats: forty-eight bytes, aligned to eight.
        assert_eq!(std::mem::size_of::<Placed>(), 48);
    }
}
