use egui_extras;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Stdio, Command};
use std::sync::RwLock;
use std::thread::sleep;
use std::io;
use std::sync::mpsc;

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)] // if we add new fields, give them default values when deserializing old state
pub struct HypertunnelApp {
    // Example stuff:
    listen_host: String,
    listen_port: u16,

    target_host: String,
    target_password: String,

    #[serde(skip)]
    engaged: bool,

    #[serde(skip)]
    child: Option<std::process::Child>,

    #[serde(skip)]
    log_lines: Vec<String>,

    #[serde(skip)]
    abort_text: Option<String>,
}

impl Default for HypertunnelApp {
    fn default() -> Self {
        Self {
            // Example stuff:
            listen_host: "127.0.0.1".to_string(),
            listen_port: 1080,
            target_host: "https://example.com".to_string(),
            target_password: "hunter2".to_string(),
            engaged: false,
            child: None,
            log_lines: Vec::new(),
            abort_text: None,
        }
    }
}

impl HypertunnelApp {
    /// Called once before the first frame.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // This is also where you can customize the look and feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.

        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        if let Some(storage) = cc.storage {
            return eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
        }

        egui_extras::install_image_loaders(&cc.egui_ctx);

        Default::default()
    }
}


impl eframe::App for HypertunnelApp {
    /// Called by the frame work to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Put your widgets into a `SidePanel`, `TopBottomPanel`, `CentralPanel`, `Window` or `Area`.
        // For inspiration and more examples, go to https://emilk.github.io/egui

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // The top panel is often a good place for a menu bar:

            egui::menu::bar(ui, |ui| {
                // NOTE: no File->Quit on web pages!
                if ui.button("Quit").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }

                egui::widgets::global_dark_light_mode_buttons(ui);
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            // The central panel the region left after adding TopPanel's and SidePanel's
            ui.heading("Hypertunnel");

            if self.engaged {
                let mut child = self.child.as_mut().unwrap();

                let do_abort = match child.try_wait() {
                    Ok(Some(status)) => {
                        println!("Child exited with: {}", status);
                        true
                    }
                    Ok(None) => false,
                    Err(e) => {
                        println!("Error: {}", e);
                        true
                    }
                };

                let mut child_stdout = child.stdout.as_mut();
                let mut child_stderr = child.stderr.as_mut();

                if ui.button("Disengage").clicked() || do_abort {
                    // Disengage the tunnel!
                    self.engaged = false;

                    // Check if we can set an abort text
                    if do_abort && (child_stderr.is_some() || child_stdout.is_some()) {
                        let mut out_lines = Vec::with_capacity(16);

                        for log in self.log_lines.iter().rev().take(10) {
                            out_lines.push(log.clone());
                        }

                        // Reverse out_lines
                        out_lines.reverse();

                        if child_stdout.is_some() {
                            // Read from stdout
                            let mut reader_stdout = BufReader::new(child_stdout.unwrap());
                            for line in reader_stdout.lines() {
                                out_lines.push(line.unwrap());
                            }
                        }

                        if child_stderr.is_some() {
                            // Read from stderr
                            let mut reader_stderr = BufReader::new(child_stderr.unwrap());
                            for line in reader_stderr.lines() {
                                out_lines.push(line.unwrap());
                            }
                        }

                        let out_text = out_lines.join("\n");
                        self.abort_text = Some(out_text.clone());

                        println!("Abort text: {}", out_text);
                    }
                    
                    // Kill child
                    child.kill().unwrap();

                    // Clear log lines
                    self.log_lines.clear();

                    self.child = None;
                } else {
                    ui.label("Program executing nominally.");
                }
            } else {
                ui.horizontal(|ui| {
                    ui.label("Listen Host: ");

                    ui.add(egui::TextEdit::singleline(&mut self.listen_host).hint_text("0.0.0.0"));
                });

                ui.horizontal(|ui| {
                    ui.label("Listen Port: ");
                    ui.add(egui::widgets::DragValue::new(&mut self.listen_port).speed(1.0));
                });

                ui.separator();

                ui.horizontal(|ui| {
                    ui.label("Target Host: ");
                    ui.text_edit_singleline(&mut self.target_host);
                });

                ui.horizontal(|ui| {
                    ui.label("Target Password:");
                    ui.add(egui::widgets::TextEdit::singleline(
                        &mut self.target_password,
                    ));
                });

                ui.separator();

                if ui.button("Engage").clicked() {
                    // Engage the tunnel!
                    let child = start_tunnel(
                        &self.listen_host,
                        self.listen_port,
                        &self.target_host,
                        &self.target_password,
                    );

                    let child = match child {
                        Ok(child) => child,
                        Err(e) => {
                            ui.label(format!("Error: {}", e));
                            return;
                        }
                    };

                    self.engaged = true;

                    self.child = Some(child);

                    self.abort_text = None;
                }

                if self.abort_text.is_some() {
                    ui.label(format!("Error: {}", self.abort_text.as_ref().unwrap()));
                }
            }

            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                egui::warn_if_debug_build(ui);
            });
        });
    }
}

fn start_tunnel(
    listen_host: &str,
    listen_port: u16,
    target_host: &str,
    target_password: &str,
) -> Result<std::process::Child, String> {
    let child = Command::new("hypertunnel")
        .arg("--listen-host")
        .arg(listen_host)
        .arg("--listen-port")
        .arg(listen_port.to_string())
        .arg("--target-host")
        .arg(target_host)
        .arg("--password")
        .arg(target_password)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    match child {
        Ok(child) => {
            return Ok(child);
        }
        Err(e) => {
            return Err(e.to_string());
        }
    }
}

fn powered_by_egui_and_eframe(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.label("Powered by ");
        ui.hyperlink_to("egui", "https://github.com/emilk/egui");
        ui.label(" and ");
        ui.hyperlink_to(
            "eframe",
            "https://github.com/emilk/egui/tree/master/crates/eframe",
        );
        ui.label(".");
    });
}
