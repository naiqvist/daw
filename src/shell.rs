//! The window, the frame, and the pass that runs after everything.
//!
//! This replaces `eframe::run_native`. Not for the sake of owning a
//! window — eframe's is fine — but because eframe has nowhere to stand
//! AFTER egui has drawn. `egui_wgpu`'s painter begins its render pass
//! straight onto the surface texture and presents; the `App` trait
//! offers `on_exit`, `clear_color` and `raw_input_hook` and nothing
//! else, and a paint callback runs INSIDE the egui pass, so it can
//! never read the finished frame.
//!
//! So the frame goes somewhere we can read it:
//!
//! ```text
//!   egui ──▶ offscreen ──▶ bloom + glass ──▶ registered screen phosphor ──▶ present
//! ```
//!
//! # What we took on
//!
//! Owning the loop means owning window creation, resize, DPI, cursors,
//! the clipboard, IME and persistence. `egui_winit::State` carries the
//! input half of that, and [`Storage`] carries the rest — deliberately
//! reading and writing eframe's OWN file, in eframe's own format, so
//! that changing the shell does not silently discard a machine's
//! preferences.
//!
//! # The frame post pass is blank
//!
//! It copies the finished frame to the swapchain unchanged. The stage keeps
//! its CRT material in a second, explicitly registered screen-only pass; the
//! old whole-frame treatment is not involved.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;

pub mod post;
pub mod screen;

/// What the shell needs from the application it runs.
///
/// Two calls, and neither knows anything about wgpu. The app is a thing
/// that draws into a `Ui` and can be asked to write its preferences
/// down; that was true under eframe and it stays true, which is the
/// point of putting the trait here rather than spreading winit through
/// the app.
pub trait Host {
    /// One egui pass. The `Ui` has no margin and no background, exactly
    /// as `eframe::App::ui` handed it over.
    fn ui(&mut self, ui: &mut egui::Ui);

    /// Machine-local preferences, on the way out and at intervals.
    fn save(&mut self, storage: &mut Storage);

    /// Once, after the context exists and before the first frame —
    /// fonts, zoom, the restored theme. Under eframe this was whatever
    /// `App::new` did with `CreationContext::egui_ctx`; the context now
    /// arrives later than the app does, so it is its own call.
    fn startup(&mut self, ctx: &egui::Context);
}

// ----------------------------------------------------------- storage ---

/// How often preferences are written while the app runs.
///
/// eframe's own interval. Preferences that only survive a clean exit are
/// preferences that do not survive a crash, and the file is small.
const SAVE_EVERY: std::time::Duration = std::time::Duration::from_secs(30);

/// A key-value store on disk, in eframe's format and at eframe's path.
///
/// `~/.local/share/daw/app.ron` on Linux, holding a
/// `HashMap<String, String>` whose values are themselves RON. Matching
/// it exactly is not nostalgia: there is a real file there with real
/// settings in it, and a new shell that quietly started fresh would look
/// like the settings had been lost.
#[derive(Default)]
pub struct Storage {
    kv: HashMap<String, String>,
    path: Option<PathBuf>,
    dirty: bool,
}

impl Storage {
    /// Load, or start empty. A missing or corrupt file is not worth
    /// failing a launch over.
    pub fn load(app_id: &str) -> Self {
        let path = storage_dir(app_id).map(|dir| dir.join("app.ron"));
        let kv = path
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| ron::from_str::<HashMap<String, String>>(&text).ok())
            .unwrap_or_default();
        Self {
            kv,
            path,
            dirty: false,
        }
    }

    pub fn get<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.kv
            .get(key)
            .and_then(|value| ron::from_str::<T>(value).ok())
    }

    pub fn set<T: serde::Serialize>(&mut self, key: &str, value: &T) {
        if let Ok(text) = ron::ser::to_string(value) {
            if self.kv.get(key).is_some_and(|old| *old == text) {
                return;
            }
            self.kv.insert(key.to_owned(), text);
            self.dirty = true;
        }
    }

    /// Write, if anything changed. Synchronous and on the UI thread:
    /// this happens twice a minute on a file of a few hundred kilobytes,
    /// and a background writer would be a thread and a race to save a
    /// millisecond nobody can perceive.
    pub fn flush(&mut self) {
        if !self.dirty {
            return;
        }
        let Some(path) = &self.path else {
            return;
        };
        if let Some(parent) = path.parent()
            && !parent.exists()
            && std::fs::create_dir_all(parent).is_err()
        {
            return;
        }
        if let Ok(text) = ron::ser::to_string(&self.kv)
            && std::fs::write(path, text).is_ok()
        {
            self.dirty = false;
        }
    }
}

/// eframe's directory rule, restated so the file lands in the same place.
fn storage_dir(app_id: &str) -> Option<PathBuf> {
    let id = app_id
        .to_lowercase()
        .replace(|c: char| c.is_ascii_whitespace(), "");
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::home_dir().map(|home| home.join(".local").join("share")))
        .map(|base| base.join(id))
}

// -------------------------------------------------------------- run ---

/// The offscreen colour target egui draws into, and the generation that
/// says when the bind group pointing at it went stale.
struct Offscreen {
    view: wgpu::TextureView,
    size: [u32; 2],
    generation: u64,
}

impl Offscreen {
    fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        size: [u32; 2],
        generation: u64,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("egui_offscreen"),
            size: wgpu::Extent3d {
                width: size[0].max(1),
                height: size[1].max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        Self {
            view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
            size,
            generation,
        }
    }
}

/// Everything the window needs once it exists.
struct Live {
    window: Arc<winit::window::Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    egui: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    offscreen: Offscreen,
    post: post::Post,
    screens: screen::Pass,
    generation: u64,
}

struct Shell<A: Host> {
    app: A,
    storage: Storage,
    title: &'static str,
    size: [f32; 2],
    min_size: [f32; 2],
    live: Option<Live>,
    last_save: std::time::Instant,
}

impl<A: Host> winit::application::ApplicationHandler for Shell<A> {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.live.is_some() {
            return;
        }
        match self.start(event_loop) {
            Ok(live) => self.live = Some(live),
            Err(err) => {
                eprintln!("daw: could not open a window: {err}");
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        let Some(live) = &mut self.live else {
            return;
        };
        // egui gets every event first, and says whether it wanted it.
        let response = live.egui.on_window_event(&live.window, &event);
        if response.repaint {
            live.window.request_redraw();
        }

        match event {
            winit::event::WindowEvent::CloseRequested => {
                // The one place preferences MUST be written: an exit
                // that dropped them would lose thirty seconds of
                // settings and look like a bug in the settings.
                self.app.save(&mut self.storage);
                self.storage.flush();
                event_loop.exit();
            }
            winit::event::WindowEvent::Resized(size) => {
                self.resize([size.width, size.height]);
            }
            winit::event::WindowEvent::RedrawRequested => {
                self.draw(event_loop);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        if let Some(live) = &self.live {
            live.window.request_redraw();
        }
    }
}

impl<A: Host> Shell<A> {
    fn resize(&mut self, size: [u32; 2]) {
        let Some(live) = &mut self.live else {
            return;
        };
        if size[0] == 0 || size[1] == 0 {
            return;
        }
        live.config.width = size[0];
        live.config.height = size[1];
        live.surface.configure(&live.device, &live.config);
        if std::env::var("DAW_SHELL_DEBUG").is_ok() {
            eprintln!(
                "shell: resized to {size:?} physical, scale={}",
                live.window.scale_factor()
            );
        }
        // The offscreen target follows the surface exactly, and the
        // generation is what tells the post pass its bind group now
        // points at a texture that no longer exists.
        live.generation += 1;
        live.offscreen = Offscreen::new(&live.device, live.config.format, size, live.generation);
    }

    fn draw(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let Some(live) = &mut self.live else {
            return;
        };
        let size = [live.config.width, live.config.height];
        if size[0] == 0 || size[1] == 0 {
            return;
        }
        if live.offscreen.size != size {
            live.generation += 1;
            live.offscreen =
                Offscreen::new(&live.device, live.config.format, size, live.generation);
        }

        // --- the egui pass, exactly as eframe ran it -------------------
        let mut raw_input = live.egui.take_egui_input(&live.window);
        // What eframe does and `take_egui_input` does not: tell egui about
        // the window itself — fullscreen, focus, its rect on the screen.
        // The surface reads `fullscreen` from here; it never sees the window.
        egui_winit::update_viewport_info(
            raw_input
                .viewports
                .entry(egui::ViewportId::ROOT)
                .or_default(),
            live.egui.egui_ctx(),
            &live.window,
            false,
        );
        let app = &mut self.app;
        let ctx = live.egui.egui_ctx().clone();
        screen::begin_frame(&ctx);
        let mut output = ctx.run_ui(raw_input, |ui| app.ui(ui));
        let screen_regions = screen::take(&ctx);
        live.egui
            .handle_platform_output(&live.window, output.platform_output);

        let pixels_per_point = live.egui.egui_ctx().pixels_per_point();
        let jobs = live
            .egui
            .egui_ctx()
            .tessellate(output.shapes, pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: size,
            pixels_per_point,
        };

        let mut encoder = live
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        // OWNED, and cleared on every path out of this function.
        // `TexturesDelta` panics in its destructor if it is dropped with
        // work still in it — a deliberate epaint tripwire, and the early
        // return below is exactly the shape it is watching for.
        let mut textures = std::mem::take(&mut output.textures_delta);
        for (id, deltas) in &textures.set {
            for delta in deltas {
                live.renderer
                    .update_texture(&live.device, &live.queue, *id, delta);
            }
        }
        let user =
            live.renderer
                .update_buffers(&live.device, &live.queue, &mut encoder, &jobs, &screen);

        // A surface that is lost, outdated or merely suboptimal is a
        // reconfigure, not a crash — the next frame gets it back. An
        // occluded or timed-out one is a frame to skip, and skipping it
        // is the whole point of being told.
        let frame = match live.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                live.surface.configure(&live.device, &live.config);
                frame
            }
            _ => {
                live.surface.configure(&live.device, &live.config);
                for id in &textures.free {
                    live.renderer.free_texture(id);
                }
                textures.clear();
                return;
            }
        };
        let target = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &live.offscreen.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            live.renderer
                .render(&mut pass.forget_lifetime(), &jobs, &screen);
        }

        // --- and then, after everything -------------------------------
        live.post.draw(
            &live.device,
            &live.queue,
            &mut encoder,
            &live.offscreen.view,
            live.offscreen.size,
            live.offscreen.generation,
            &target,
        );
        live.screens.draw(
            &live.device,
            &live.queue,
            &mut encoder,
            &live.offscreen.view,
            live.offscreen.generation,
            &target,
            size,
            pixels_per_point,
            &screen_regions,
        );

        live.queue
            .submit(user.into_iter().chain(std::iter::once(encoder.finish())));
        live.window.pre_present_notify();
        live.queue.present(frame);

        for id in &textures.free {
            live.renderer.free_texture(id);
        }
        textures.clear();

        if self.last_save.elapsed() >= SAVE_EVERY {
            self.app.save(&mut self.storage);
            self.storage.flush();
            self.last_save = std::time::Instant::now();
        }

        // The app asks for frames when it has something moving — a
        // meter falling, a spring settling. Anything else waits.
        let delay = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|viewport| viewport.repaint_delay);
        match delay {
            Some(delay) if delay.is_zero() => {
                if let Some(live) = &self.live {
                    live.window.request_redraw();
                }
                event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
            }
            Some(delay) => {
                event_loop.set_control_flow(match std::time::Instant::now().checked_add(delay) {
                    Some(at) => winit::event_loop::ControlFlow::WaitUntil(at),
                    None => winit::event_loop::ControlFlow::Wait,
                });
            }
            None => event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait),
        }
    }

    fn start(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
    ) -> Result<Live, Box<dyn std::error::Error>> {
        let attributes = winit::window::Window::default_attributes()
            .with_title(self.title)
            .with_inner_size(winit::dpi::LogicalSize::new(self.size[0], self.size[1]))
            .with_min_inner_size(winit::dpi::LogicalSize::new(
                self.min_size[0],
                self.min_size[1],
            ));
        let window = Arc::new(event_loop.create_window(attributes)?);

        // The display handle rides along so wgpu can pick a backend that
        // matches the compositor we were actually given.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(
            Box::new(window.clone()),
        ));
        let surface = instance.create_surface(window.clone())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            ..Default::default()
        }))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("daw"),
                required_features: wgpu::Features::empty(),
                required_limits: adapter.limits(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
                ..Default::default()
            }))?;

        let physical = window.inner_size();
        let size = [physical.width.max(1), physical.height.max(1)];
        let caps = surface.get_capabilities(&adapter);
        // sRGB where it is offered. The offscreen target takes the SAME
        // format, so sampling it decodes to linear and writing back
        // re-encodes — which is what makes the bypass exact rather than
        // approximately exact.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .or_else(|| caps.formats.first().copied())
            .ok_or("the surface offers no format")?;
        let mut config = surface
            .get_default_config(&adapter, size[0], size[1])
            .ok_or("the surface is not usable with this adapter")?;
        config.format = format;
        config.usage |= wgpu::TextureUsages::RENDER_ATTACHMENT;
        surface.configure(&device, &config);

        let egui_ctx = egui::Context::default();
        let egui = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            Some(device.limits().max_texture_dimension_2d as usize),
        );
        let renderer = egui_wgpu::Renderer::new(&device, format, Default::default());
        let post = post::Post::new(&device, format);
        let screens = screen::Pass::new(&device, format);
        let offscreen = Offscreen::new(&device, format, size, 0);

        if std::env::var("DAW_SHELL_DEBUG").is_ok() {
            eprintln!(
                "shell: format={format:?} size={size:?} scale={} zoom={}",
                window.scale_factor(),
                egui_ctx.zoom_factor(),
            );
        }

        self.app.startup(&egui_ctx);

        Ok(Live {
            window,
            surface,
            config,
            device,
            queue,
            egui,
            renderer,
            offscreen,
            post,
            screens,
            generation: 0,
        })
    }
}

/// Open the window and run until it closes.
///
/// `build` is handed the storage before anything is drawn, the way
/// `eframe::CreationContext` was.
pub fn run<A: Host>(
    title: &'static str,
    size: [f32; 2],
    min_size: [f32; 2],
    build: impl FnOnce(&Storage) -> A,
) -> Result<(), Box<dyn std::error::Error>> {
    let storage = Storage::load(title);
    let app = build(&storage);
    let event_loop = winit::event_loop::EventLoop::new()?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
    let mut shell = Shell {
        app,
        storage,
        title,
        size,
        min_size,
        live: None,
        last_save: std::time::Instant::now(),
    };
    event_loop.run_app(&mut shell)?;
    Ok(())
}
