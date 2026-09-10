//! Native visual preview pixels; all document commands stay in the core.
use super::super::Stage;
use crate::visuals::{self, gpu};
use std::sync::atomic::Ordering;
impl Stage {
    pub(super) fn show_visuals(&mut self, ctx: &egui::Context) {
        if let Some(job) = &mut self.visual_export
            && let Some(result) = job.poll()
        {
            let cancelled = job.cancel.load(Ordering::Relaxed);
            self.visual_export = None;
            self.notice = Some(if cancelled {
                if self.visual_preview.is_none() {
                    "VISUAL OFF · no preview / worker"
                } else {
                    "VIDEO CANCELLED"
                }
                .into()
            } else {
                result.unwrap_or_else(|e| format!("REFUSED video export · {e}"))
            });
        }
        let progress = self
            .visual_export
            .as_ref()
            .map(|j| j.progress.load(Ordering::Relaxed));
        let stopping = self
            .visual_export
            .as_ref()
            .is_some_and(|j| j.cancel.load(Ordering::Relaxed));
        let mut off = false;
        let mut edit = None;
        if let Some(preview) = &mut self.visual_preview {
            // A parked arrangement cursor is the scrub position; rolling
            // always follows the host's fractional audio-clock mirror.
            let tick = if self.song_view && !self.transport.motion().is_rolling() {
                self.arrangement.tick as f64
            } else {
                self.transport.precise_tick()
            };
            let mut open = true;
            egui::Window::new("Visual score").id(egui::Id::new("visual-score-preview"))
                .open(&mut open).default_width(720.0).resizable(true).fade_in(false).fade_out(false).show(ctx,|ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Off").clicked(){off=true;}
                        let locked=preview.compiled.score().locked;
                        if ui.button(if locked{"Unlock"}else{"Lock"}).clicked(){edit=Some(if locked{"visual unlock"}else{"visual lock"});}
                        ui.label(format!("TRANSPORT · {:.2} beats",tick/48.0));
                        if let Some(p)=progress {ui.label(format!("VIDEO {}%",p/10));}
                    });
                    let width=ui.available_width().clamp(160.0,1600.0);
                    let (rect,_)=ui.allocate_exact_size(egui::vec2(width,width*9.0/16.0),egui::Sense::hover());
                    let frame=preview.frame(tick,rect.aspect_ratio());
                    ui.painter().add(egui_wgpu::Callback::new_paint_callback(rect,gpu::Callback{frame,gpu:preview.gpu.clone()}));
                    ui.label("Ctrl+Shift+P → visual …    •    48 ticks / beat    •    Lock protects edits; Off releases rendering");
                    ui.collapsing("Clips, placement and linked parameters",|ui| {
                        egui::ScrollArea::vertical().max_height(200.0).show(ui,|ui| {
                            let score=preview.compiled.score();
                            for (i,p) in score.arrangement.iter().enumerate() {
                                ui.label(format!("{} · {} · tick {} → {} · {}",i+1,p.clip,p.at,p.at+p.length_ticks,if p.repeat{"repeat"}else{"once"}));
                            }
                            for c in &score.clips {
                                ui.collapsing(format!("{} · {} ticks · {} layers",c.id,c.length_ticks,c.layers.len()),|ui| {
                                    for l in &c.layers {
                                        ui.collapsing(format!("{} · {:?} / {:?}",l.id,l.primitive,l.blend),|ui| {
                                            ui.label(format!("visual set {} {} name=value; name=value",c.id,l.id));
                                            for (i,p) in visuals::PARAMS.iter().enumerate() {
                                                ui.label(format!("{} = {:.3} · {}..{}",p.0,l.params[i],p.1,p.2));
                                            }
                                            for a in &l.automation {ui.label(format!("{} · {} parameter locks",a.param,a.keys.len()));}
                                            for (i,m) in l.modulation.iter().enumerate() {ui.label(format!("route {} · {:?} → {} × {}",i+1,m.source,m.param,m.depth));}
                                        });
                                    }
                                });
                            }
                        });
                    });
                });
            off |= !open;
        } else if let Some(p) = progress {
            egui::Window::new("Video export")
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(if stopping {
                        "Stopping — waiting for worker to exit".into()
                    } else {
                        format!("Audio + video: {}%", p / 10)
                    });
                    if ui.button("Cancel / Off").clicked() {
                        off = true;
                    }
                });
        }
        if off {
            self.apply_visual_command("visual off");
        } else if let Some(command) = edit {
            self.apply_visual_command(command);
        }
    }
}
