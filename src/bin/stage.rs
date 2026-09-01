//! `stage` — the third UI, from nothing.
//!
//!     cargo run --bin stage
//!
//! Its own binary on purpose. `daw` is a working DAW and stays that way
//! while this is built beside it: nothing here touches `main.rs`, and
//! there is no third state to invent on a key that already flips between
//! two. When the stage is ready to be the app, it becomes the default —
//! until then the two are simply different programs over one library.
//!
//! What it inherits for free, because they are frame-independent: the
//! model, the intent vocabulary, the device cards and the design system.
//! What it inherits deliberately NOTHING of: the layout, the key map, and
//! the panel structure of either frame before it.

use daw::install_fonts;
use daw::ui::stage::Stage;
use daw::ui::theme::Theme;
use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("daw — stage")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([720.0, 480.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "daw-stage",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

struct App {
    stage: Stage,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // The fonts and the theme are the design system, and they are the
        // one part of the old surfaces that carries over unchanged — they
        // encode the aesthetic charter rather than any layout.
        install_fonts(&cc.egui_ctx);
        Theme::dark().apply(&cc.egui_ctx);
        Self {
            stage: Stage::new(),
        }
    }
}

impl eframe::App for App {
    /// Black, and meant. The ground is black because the theme says so,
    /// not because nothing ran.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 1.0]
    }

    /// eframe 0.36 hands the app a `Ui` rather than a `Context` and a
    /// panel to build, so there is nothing between the window and the
    /// stage. That is the whole surface.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.stage.show(ui);
    }
}
