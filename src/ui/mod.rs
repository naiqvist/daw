//! The real application's UI system.
//!
//! # Layer contracts (mechanically enforced — see tests below)
//!
//! Dependency direction is strict and one-way:
//!
//! ```text
//! tokens, vm, action  <-  theme, prefs  <-  kit, keymap  <-  host  <-  panels  <-  app
//! ```
//!
//! 0. `vm` (ViewState + domain limits) and `action` (UiAction) are shared
//!    vocabulary at the tokens level: plain data, no engine imports. The
//!    ACTION LOOP: app builds ViewState from engine telemetry -> panels read
//!    it and return UiActions -> app (sole Engine owner) translates them.
//!    One-way flow; panels are pure functions of state.
//! 1. `tokens` depends on nothing. Pure consts (plus `Density`, which is a
//!    style scale both `theme` and `action` must be able to name).
//! 2. `theme` may use tokens + egui types. Never kit, never panels.
//!    `prefs` is plain serde data at the same level: no egui, no engine.
//! 3. `kit` may use tokens + theme + egui. Never panels. It is the ONLY
//!    place raw numbers reach egui, and — together with `device`, its
//!    sibling at this layer — the only place custom painting
//!    (`ui.painter()`) lives. `device` holds the param-wired synth/FX
//!    widgets (knobs, faders, XY pads, envelopes, spectra) and may call
//!    `kit`; `kit` must never know `device`. `keymap` sits beside them:
//!    the key-gesture table, egui types in, `UiAction`s out. `skin`, the
//!    theme window, is another sibling at this layer: it previews the two
//!    authored house schemes and paints the picker.
//! 4. `host` owns the panel registry, dock layout, and `PanelCx`. It knows
//!    the `Panel` TRAIT; it must never name a concrete panel.
//! 5. `panels` speak tokens/theme/kit/host + egui *layout* (Ui, horizontal,
//!    ...). No literals, no painter, and NO ENGINE: panels never import
//!    `crate::audio`. A panel renders state it is handed and returns what
//!    the user asked for; the app layer owns the Engine and translates.
//!    This is the UI's red-zone rule — a panel wired straight to the audio
//!    thread is the bug we cannot cheaply find later.
//! 6. `main.rs` (app) registers panels, owns Engine + Theme + PanelHost,
//!    pumps snapshots in and actions out.
//!
//! `gallery` is dev-facing and sits at the app level: it may use everything
//! below it, but it is still barred from the engine, because its whole point
//! is proving panels render without one.
//!
//! The lab binary is exempt from all of this: it is a harness, not the app.
//!
//! # Adding a panel
//!
//! One file in `panels/`, one `pub mod` line, one `host.register(...)` in
//! `main.rs`. Dock side, size, frame, show order, visibility, tabbing, and
//! preference persistence all follow from the trait — see `host`.
//!
//! # The persistence boundary: UI prefs != project
//!
//! Machine-local preferences — window size and position, theme choice,
//! density, panel layout, last-open view — persist on THIS MACHINE (eframe
//! storage / a config-dir file) and never enter a project file. A project
//! file holds musical content only, and must open identically on a machine
//! that has never seen this one's window layout. Sibling of the transport
//! rule "musical time only, never sample positions": both are answers to
//! the same question — what IS the document? If a value would still matter
//! after emailing the project to a stranger, it belongs in the project;
//! otherwise it is a preference.
//!
//! Violations are TEST FAILURES, not review comments.

pub mod action;
pub mod affordance;
pub mod chrome;
pub mod device;
pub mod gallery;
pub mod glyph;
pub mod host;
pub mod hud;
pub mod keymap;
pub mod kit;
pub mod legibility;
pub mod nav_cursor;
pub mod palette;
pub mod panels;
pub mod prefs;
/// The new application frame, built independently from the legacy panels.
pub mod redesign;
pub mod sequencer;
/// The replacement Session surface. Registered now that `session_bridge`
/// exists to adapt it — before the adapters, this would have been a
/// second authority for what plays.
pub mod session_next;
pub mod skin;
pub mod stage;
pub mod theme;
pub mod tokens;
pub mod vm;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    const UI_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/ui");

    fn module_files(dir: &str) -> Vec<std::path::PathBuf> {
        let mut files: Vec<_> = std::fs::read_dir(format!("{UI_ROOT}/{dir}"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("rs"))
            .filter(|p| p.file_stem().and_then(|s| s.to_str()) != Some("mod"))
            .collect();
        files.sort();
        files
    }

    fn panel_files() -> Vec<std::path::PathBuf> {
        module_files("panels")
    }

    /// The token system's teeth: scan every panel source file for numeric
    /// literals. Allowed: 0, 1, 0.0, 1.0, 0.5 (identity/center), literals in
    /// `const` items, and lines carrying a `// magic: <reason>` tag — the
    /// documented, greppable escape hatch. Everything else fails, by
    /// file:line, in plain `cargo test`.
    #[test]
    fn panels_contain_no_magic_numbers() {
        let allowed = ["0", "1", "0.0", "1.0", "0.5"];
        let mut violations = Vec::new();

        for path in panel_files() {
            let text = std::fs::read_to_string(&path).unwrap();
            for (ln, raw) in text.lines().enumerate() {
                let line = raw.trim();
                // Comments, escape hatch, and const items are exempt.
                if line.starts_with("//") || raw.contains("// magic:") {
                    continue;
                }
                if line.starts_with("const ") || line.starts_with("pub const ") {
                    continue;
                }
                // Strip string literals so format specs don't false-positive.
                let mut code = String::with_capacity(raw.len());
                let mut in_str = false;
                let mut prev = '\0';
                for c in raw.chars() {
                    match c {
                        '"' if prev != '\\' => {
                            in_str = !in_str;
                            code.push(' ');
                        }
                        _ if in_str => code.push(' '),
                        _ => code.push(c),
                    }
                    prev = c;
                }
                // Strip trailing line comment.
                let code = code.split("//").next().unwrap_or("");

                // Extract numeric literals.
                let mut lit = String::new();
                let mut prev_ch = '\0';
                let mut push_violation = |lit: &str| {
                    if !lit.is_empty() && !allowed.contains(&lit) {
                        violations.push(format!(
                            "{}:{}: literal `{}` — use a token (or tag `// magic: <why>`)",
                            path.file_name().unwrap().to_string_lossy(),
                            ln + 1,
                            lit
                        ));
                    }
                };
                for c in code.chars() {
                    let in_ident = prev_ch.is_alphanumeric() || prev_ch == '_';
                    let extends_literal = (c.is_ascii_digit() && (!lit.is_empty() || !in_ident))
                        || (c == '.' && !lit.is_empty() && !lit.contains('.'));
                    if extends_literal {
                        lit.push(c);
                    } else {
                        push_violation(lit.trim_end_matches('.'));
                        lit.clear();
                    }
                    prev_ch = c;
                }
                push_violation(lit.trim_end_matches('.'));
            }
        }

        assert!(
            violations.is_empty(),
            "magic numbers in panels:\n{}",
            violations.join("\n")
        );
    }

    /// The layer contracts, as import scans. Same philosophy as the literal
    /// scan: the dependency direction is a rule a compiler cannot check
    /// (same crate), so a test reads the source and checks it.
    #[test]
    fn ui_layers_respect_their_contracts() {
        let read = |rel: &str| std::fs::read_to_string(format!("{UI_ROOT}/{rel}")).unwrap();
        let mut violations: Vec<String> = Vec::new();
        let mut forbid = |file: &str, text: &str, needle: &str, why: &str| {
            for (ln, line) in text.lines().enumerate() {
                let code = line.split("//").next().unwrap_or("");
                if code.contains(needle) {
                    violations.push(format!("{file}:{}: `{needle}` — {why}", ln + 1));
                }
            }
        };

        // tokens: depends on nothing.
        let t = read("tokens.rs");
        forbid("tokens.rs", &t, "use crate", "tokens depend on nothing");
        forbid("tokens.rs", &t, "use eframe", "tokens depend on nothing");

        // vm + action: shared vocabulary — plain data, never the engine.
        for f in ["vm.rs", "action.rs"] {
            let text = read(f);
            forbid(
                f,
                &text,
                "crate::audio",
                "vocabulary must not know the engine",
            );
            forbid(f, &text, "use eframe", "vocabulary is plain data, not UI");
        }

        // prefs: plain serde data. Not UI, not engine, and — the boundary
        // this whole file is about — not the project document.
        let p = read("prefs.rs");
        forbid(
            "prefs.rs",
            &p,
            "crate::audio",
            "preferences are not the project",
        );
        forbid(
            "prefs.rs",
            &p,
            "use eframe",
            "preferences are plain data, not UI",
        );

        // theme: tokens + egui only.
        let th = read("theme.rs");
        forbid("theme.rs", &th, "crate::ui::kit", "theme must not know kit");
        forbid(
            "theme.rs",
            &th,
            "crate::ui::panels",
            "theme must not know panels",
        );
        forbid(
            "theme.rs",
            &th,
            "crate::audio",
            "theme must not know the engine",
        );

        // kit + keymap: may use tokens/theme/vm/action + egui. Never panels,
        // never the host, never the engine. kit also never knows device —
        // between the layer-3 siblings the dependency is one-way.
        for f in ["kit.rs", "keymap.rs", "palette.rs"] {
            let text = read(f);
            forbid(f, &text, "crate::ui::panels", "must not know panels");
            forbid(f, &text, "crate::ui::host", "must not know the host");
            forbid(f, &text, "crate::audio", "must not know the engine");
            forbid(f, &text, "crate::ui::device", "kit must not know device");
        }

        // device: kit's sibling — same restrictions (device MAY use kit).
        for path in module_files("device") {
            let name = format!("device/{}", path.file_name().unwrap().to_string_lossy());
            let text = std::fs::read_to_string(&path).unwrap();
            forbid(&name, &text, "crate::ui::panels", "must not know panels");
            forbid(&name, &text, "crate::ui::host", "must not know the host");
            forbid(&name, &text, "crate::audio", "must not know the engine");
        }

        // host: knows the Panel TRAIT, never a concrete panel, never the engine.
        let h = read("host.rs");
        forbid(
            "host.rs",
            &h,
            "crate::ui::panels",
            "the host must not name a concrete panel",
        );
        forbid(
            "host.rs",
            &h,
            "crate::audio",
            "the host must not know the engine",
        );

        // gallery: app-level, but still no engine — its point is that panels
        // render without one.
        let g = read("gallery.rs");
        forbid(
            "gallery.rs",
            &g,
            "crate::audio",
            "the gallery proves panels need no engine",
        );

        // panels: no engine, no custom painting, no raw color, no persistence.
        for path in panel_files() {
            let name = format!("panels/{}", path.file_name().unwrap().to_string_lossy());
            let text = std::fs::read_to_string(&path).unwrap();
            forbid(
                &name,
                &text,
                "crate::audio",
                "panels never touch the engine — the app layer translates",
            );
            forbid(
                &name,
                &text,
                "daw::audio",
                "panels never touch the engine — the app layer translates",
            );
            forbid(&name, &text, ".painter(", "custom painting lives in kit");
            forbid(
                &name,
                &text,
                "Color32::",
                "colors come from the theme, by role",
            );
            forbid(
                &name,
                &text,
                "crate::ui::prefs",
                "panels do not persist themselves — the app saves prefs",
            );
        }

        assert!(
            violations.is_empty(),
            "layer contract violations:\n{}",
            violations.join("\n")
        );
    }

    /// The design system's teeth: a device card CANNOT ship with ad-hoc
    /// spacing OR shape, because no device file except `design.rs` may
    /// construct a frame, name a margin, or reach for a radius token. Every surface arrives from `device::design`
    /// pre-margined on the 4px grid — which is what keeps card padding
    /// visually consistent across every device anyone ever writes.
    #[test]
    fn device_margins_come_from_the_design_system() {
        let mut violations: Vec<String> = Vec::new();
        for path in module_files("device") {
            let file = path.file_name().unwrap().to_string_lossy().to_string();
            // `grid.rs` is NOT declared in `device/mod.rs` and never has
            // been: it references a `control::CELL` token that does not
            // exist, so it has never compiled. It is exempt only so that
            // a file which is not part of the build cannot fail a rule
            // about the build — the exemption is a marker, not a pardon.
            // It should be finished or deleted.
            if file == "design.rs" || file == "grid.rs" {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for (ln, line) in text.lines().enumerate() {
                let code = line.split("//").next().unwrap_or("");
                // `radius::` joins the list because it drifted exactly
                // the way margins would have: the filter display had
                // square corners while its own thumbnail had round ones,
                // and nothing said which was right because nothing had
                // said anything. Corner radius is spacing's sibling —
                // shape decided per widget is shape decided nowhere.
                for needle in [
                    "Frame::new",
                    "inner_margin(",
                    "outer_margin(",
                    "Margin::",
                    "radius::",
                ] {
                    if code.contains(needle) {
                        violations.push(format!(
                            "device/{file}:{}: `{needle}` — frames and margins come from \
                             device::design, nowhere else",
                            ln + 1
                        ));
                    }
                }
            }
        }
        assert!(
            violations.is_empty(),
            "design system violations:\n{}",
            violations.join("\n")
        );
    }

    /// The size contract's teeth: every device WIDGET module publishes a
    /// `footprint`, beside the draw function it describes.
    ///
    /// A widget that draws without one cannot be laid out into an exact
    /// rectangle — a container is left guessing how much room its label
    /// and readout need, and guessing is what clips text. Adding a widget
    /// module without a footprint fails here rather than at the far end,
    /// where the symptom is a truncated word in one theme at one density.
    ///
    /// Only widget modules are asked. `design`, `metrics`, `param`,
    /// `bezier` and `adjust` draw nothing; `card` is the container that
    /// CONSUMES footprints; the device cards compose widgets and take
    /// their size from what they hold.
    #[test]
    fn every_device_widget_publishes_a_footprint() {
        // Everything that is not a widget, and why it is exempt.
        const NOT_WIDGETS: &[&str] = &[
            "mod.rs",     // the module's own doc
            "design.rs",  // frames and spacing, no widget
            "metrics.rs", // the contract vocabulary itself
            "param.rs",   // the wiring vocabulary, no drawing
            "bezier.rs",  // pure curve math, no egui at all
            "adjust.rs",  // shared input helper, draws nothing
            "card.rs",    // the container that consumes footprints
            "grid.rs",    // squiggle block, sized by its own cell token
            "synth.rs",   // a card: its size is the sum of what it holds
            "reverb.rs",
            "poly.rs",
            "loom.rs", // a card: its size is the sum of what it holds
            "sat.rs",
            "echo.rs",
            "eq.rs",
            "probe.rs",  // the headless pointer harness: test-only, draws nothing
            "glue.rs",   // a card: its size is the sum of what it holds
            "kick.rs",   // ditto
            "haze.rs",   // a card: its size is the widest of its four pages
            "clamp.rs",  // a card: its size is the sum of what it holds
            "flint.rs",  // ditto
            "sibyl.rs",  // ditto
            "ferric.rs", // ditto
            "umbra.rs",  // ditto
            "tone.rs",   // ditto
            "sigil.rs",  // ditto
            "tine.rs",   // ditto
            "scomp.rs",  // ditto
            "stab.rs",   // ditto
            "quad.rs",   // ditto
            "gauge.rs",  // ditto
            "prism.rs",  // a card: its size is the sum of what it holds
            "limiter.rs", // ditto
            "lofi.rs",   // ditto
            "sheen.rs",  // ditto
            "disperser.rs", // ditto
            "tilt.rs",   // ditto
            "phaser.rs", // ditto
            "gate.rs",   // ditto
            "strip.rs",  // ditto
            "resyn.rs",  // ditto
            "acid.rs",   // ditto
            "rack.rs",   // a container: its size is what it holds
            "modulato.rs", // ditto
            "sampler.rs", // ditto
            "snare.rs",  // ditto
            "tom.rs",    // ditto
            "hat.rs",    // ditto
            "handclap.rs", // ditto
            "utility.rs", // ditto
        ];
        let mut missing: Vec<String> = Vec::new();
        for path in module_files("device") {
            let file = path.file_name().unwrap().to_string_lossy().to_string();
            if NOT_WIDGETS.contains(&file.as_str()) {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            if !text.contains("pub fn footprint(") {
                missing.push(format!(
                    "device/{file}: a widget module must publish `pub fn footprint(..)` \
                     so a container can reserve room for its text before it draws"
                ));
            }
        }
        assert!(
            missing.is_empty(),
            "size contract violations:\n{}",
            missing.join("\n")
        );
    }

    /// EVERY DEVICE CARD IS THE SAME HEIGHT.
    ///
    /// A rack is a column of cards, and one that is shorter than its
    /// neighbours reads as broken rather than as compact. The rule is
    /// easy to keep and easy to lose: `card::card` defaults to the SHORT
    /// body, so a card written the obvious way comes out a third short —
    /// which is exactly how `synth.rs` ended up that way and stayed
    /// there.
    ///
    /// So the height is not a matter of taste per card: every module
    /// that draws one passes `DEVICE_TALL_H`, and this counts them.
    #[test]
    fn every_device_card_is_the_same_height() {
        // Containers and the card kit itself size themselves to what
        // they hold, and are the only modules allowed a say.
        const NOT_A_CARD: &[&str] = &["card.rs", "rack.rs", "probe.rs"];
        let mut wrong: Vec<String> = Vec::new();
        for path in module_files("device") {
            let file = path.file_name().unwrap().to_string_lossy().to_string();
            if NOT_A_CARD.contains(&file.as_str()) {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            // The short helper, which defaults to `DEVICE_H`.
            for call in ["card::card(", "card::tabbed_card("] {
                if text.contains(call) {
                    wrong.push(format!(
                        "device/{file}: uses `{call}`, which defaults to the SHORT body — \
                         pass DEVICE_TALL_H through `card_sized` instead"
                    ));
                }
            }
            // And any sized call must ask for the tall one.
            for (i, line) in text.lines().enumerate() {
                if line.contains("card_sized(") && line.contains("control::DEVICE_H") {
                    wrong.push(format!("device/{file}:{}: draws a card at DEVICE_H", i + 1));
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "cards at more than one height:\n{}",
            wrong.join("\n")
        );
    }

    /// The margin rhythm itself: spacing tokens sit on the 4px grid and
    /// ascend strictly, so "one step roomier" always means something.
    ///
    /// `XXS` is deliberately NOT in this scale. It is a half-step for
    /// compact chrome, and the exemption is written down here so that
    /// adding a SECOND off-grid value has to argue with this comment
    /// first — which is the whole point of having a grid.
    #[test]
    fn spacing_tokens_keep_the_grid() {
        use crate::ui::tokens::space;
        let scale = [
            space::XS,
            space::SM,
            space::MD,
            space::LG,
            space::XL,
            space::XXL,
        ];
        for v in scale {
            assert!(
                v % 4.0 == 0.0 && v > 0.0,
                "spacing token {v} is off the 4px grid"
            );
        }
        for w in scale.windows(2) {
            assert!(
                w[0] < w[1],
                "spacing scale must ascend: {} !< {}",
                w[0],
                w[1]
            );
        }
        // The one exemption, pinned: a half-step, below the smallest
        // scale value, and exactly half of it.
        const { assert!(space::XXS * 2.0 == space::XS) };
        const { assert!(space::XXS < space::XS) };
    }

    /// A `.rs` file in `panels/` that nobody declared compiles to nothing and
    /// fails silently forever. Every panel file is a module, and every panel
    /// module implements the trait.
    #[test]
    fn every_panel_file_is_a_declared_panel() {
        let decls = std::fs::read_to_string(format!("{UI_ROOT}/panels/mod.rs")).unwrap();
        let mut violations = Vec::new();

        for path in panel_files() {
            let stem = path.file_stem().unwrap().to_string_lossy().to_string();
            if !decls.contains(&format!("pub mod {stem};")) {
                violations.push(format!(
                    "panels/{stem}.rs is not declared in panels/mod.rs — it never compiles"
                ));
            }
            let text = std::fs::read_to_string(&path).unwrap();
            if !text.contains("impl Panel for") {
                violations.push(format!(
                    "panels/{stem}.rs declares no `impl Panel for` — panels register or they \
                     are not panels"
                ));
            }
        }

        assert!(violations.is_empty(), "{}", violations.join("\n"));
    }
}
