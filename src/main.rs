// src/main.rs

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::Window,
};

// eGUI imports
use egui::{
    Color32, Context, FontFamily, FontId, Rounding, ScrollArea,
    Stroke, Style, TextStyle, ViewportId, Visuals, 
    Vec2,
};
use egui_wgpu::ScreenDescriptor;
use egui::style::Spacing;
use egui_wgpu::Renderer as EguiRenderer;
use egui_winit::State as EguiWinitState;

/// Message enum for communication between async tasks and the UI thread.
#[derive(Debug)]
enum AppMessage {
    TaskCompleted(String),
    NewLine(String),
}

/// Manages the terminal's text content with scrolling support.
struct TextBuffer {
    lines: Vec<String>,
    max_lines: usize,
    scroll_position: usize,
}

impl TextBuffer {
    fn new(max_lines: usize) -> Self {
        Self {
            lines: Vec::with_capacity(max_lines),
            max_lines,
            scroll_position: 0,
        }
    }

    fn add_line(&mut self, line: String) {
        if self.lines.len() >= self.max_lines {
            self.lines.remove(0);
            if self.scroll_position > 0 {
                self.scroll_position -= 1;
            }
        }
        self.lines.push(line);
        // Auto-scroll to bottom when new line is added
        self.scroll_position = self.lines.len().saturating_sub(self.max_lines);
    }

    fn scroll_up(&mut self) {
        if self.scroll_position > 0 {
            self.scroll_position -= 1;
        }
    }

    fn scroll_down(&mut self) {
        let max_scroll = self.lines.len().saturating_sub(self.max_lines);
        if self.scroll_position < max_scroll {
            self.scroll_position += 1;
        }
    }

    fn scroll_to_top(&mut self) {
        self.scroll_position = 0;
    }

    fn scroll_to_bottom(&mut self) {
        self.scroll_position = self.lines.len().saturating_sub(self.max_lines);
    }

    fn visible_lines(&self) -> &[String] {
        let start = self.scroll_position;
        let end = (start + self.max_lines).min(self.lines.len());
        if start < self.lines.len() {
            &self.lines[start..end]
        } else {
            &[]
        }
    }

    fn is_at_bottom(&self) -> bool {
        self.scroll_position >= self.lines.len().saturating_sub(self.max_lines)
    }
}

/// Holds the entire application state.
struct AppState {
    text_buffer: TextBuffer,
    status_message: String,
    message_receiver: mpsc::Receiver<AppMessage>,
    command_input: String,
}

/// Creates the "Hacker Theme" as specified in THEMING_SYSTEM.md.
fn create_hacker_theme() -> Style {
    let hacker_green = Color32::from_rgb(0, 255, 68);
    let background_dark = Color32::from_rgb(10, 10, 10);
    let mid_gray = Color32::from_rgb(60, 60, 60);
    let light_gray = Color32::from_rgb(100, 100, 100);

    let mut style = Style::default();

    style.visuals = Visuals {
        dark_mode: true,
        override_text_color: Some(hacker_green),
        panel_fill: background_dark,
        window_rounding: Rounding::ZERO,
        window_stroke: Stroke::new(1.0, mid_gray),
        selection: egui::style::Selection {
            bg_fill: Color32::from_rgba_premultiplied(
                hacker_green.r(),
                hacker_green.g(),
                hacker_green.b(),
                50,
            ),
            stroke: Stroke::new(1.0, hacker_green),
        },
        ..Visuals::dark()
    };

    style.spacing = Spacing {
        item_spacing: Vec2::new(8.0, 8.0),
        ..Spacing::default()
    };

    style.text_styles = [
        (
            TextStyle::Heading,
            FontId::new(24.0, FontFamily::Monospace),
        ),
        (TextStyle::Body, FontId::new(16.0, FontFamily::Monospace)),
        (TextStyle::Button, FontId::new(16.0, FontFamily::Monospace)),
        (
            TextStyle::Monospace,
            FontId::new(16.0, FontFamily::Monospace),
        ),
        (TextStyle::Small, FontId::new(12.0, FontFamily::Monospace)),
    ]
    .into();

    let widget_visuals = &mut style.visuals.widgets;
    widget_visuals.inactive = egui::style::WidgetVisuals {
        bg_fill: mid_gray,
        fg_stroke: Stroke::new(1.0, hacker_green),
        rounding: Rounding::ZERO,
        bg_stroke: Stroke::new(1.0, hacker_green),
        ..widget_visuals.inactive
    };
    widget_visuals.hovered = egui::style::WidgetVisuals {
        bg_fill: light_gray,
        fg_stroke: Stroke::new(2.0, hacker_green),
        bg_stroke: Stroke::new(1.0, hacker_green),
        ..widget_visuals.hovered
    };
    widget_visuals.active = egui::style::WidgetVisuals {
        bg_fill: background_dark,
        fg_stroke: Stroke::new(2.0, hacker_green),
        bg_stroke: Stroke::new(2.0, hacker_green),
        ..widget_visuals.active
    };

    style
}

/// Main application struct that manages all resources
struct NeoTermApp {
    event_loop: Option<EventLoop<()>>,
    window: Option<Arc<Window>>,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    adapter: Option<wgpu::Adapter>,
    config: Option<wgpu::SurfaceConfiguration>,
    egui_ctx: Context,
    egui_state: Option<EguiWinitState>,
    egui_renderer: Option<EguiRenderer>,
    app_state: AppState,
    _message_sender: mpsc::Sender<AppMessage>, // Keep sender alive
}

impl NeoTermApp {
    async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        env_logger::init();

        let event_loop = EventLoop::new()?;
        let window = Arc::new(Window::new(&event_loop)?);
        window.set_title("Neo-Term");

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .ok_or("Failed to find suitable adapter")?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default(), None)
            .await?;

        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: wgpu::TextureFormat::Bgra8UnormSrgb, // Default format
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        let egui_ctx = Context::default();
        let egui_state = EguiWinitState::new(egui_ctx.clone(), ViewportId::ROOT, &event_loop, None, None);
        let egui_renderer = EguiRenderer::new(&device, wgpu::TextureFormat::Bgra8UnormSrgb, None, 1);
        egui_ctx.set_style(create_hacker_theme());

        let (message_sender, message_receiver) = mpsc::channel::<AppMessage>(1000);

        let mut app_state = AppState {
            text_buffer: TextBuffer::new(1000),
            status_message: "STATUS: System nominal.".to_string(),
            message_receiver,
            command_input: String::new(),
        };

        // Initialize with ASCII art
        let ascii_art = [
            "███╗   ██╗███████╗ ██████╗",
            "████╗  ██║██╔════╝██╔═══██╗",
            "██╔██╗ ██║█████╗  ██║   ██║",
            "██║╚██╗██║██╔══╝  ██║   ██║",
            "██║ ╚████║███████╗╚██████╔╝",
            "╚═╝  ╚═══╝╚══════╝ ╚═════╝ ",
            "",
            "Welcome to Neo-Term. Standby for commands."
        ];
        for line in &ascii_art {
            app_state.text_buffer.add_line(line.to_string());
        }

        Ok(Self {
            event_loop: Some(event_loop),
            window: Some(window),
            device: Some(device),
            queue: Some(queue),
            adapter: Some(adapter),
            config: Some(config),
            egui_ctx,
            egui_state: Some(egui_state),
            egui_renderer: Some(egui_renderer),
            app_state,
            _message_sender: message_sender,
        })
    }

    fn run(mut self) -> Result<(), Box<dyn std::error::Error>> {
        let event_loop = self.event_loop.take().unwrap();
        let window = self.window.take().unwrap();
        let device = self.device.take().unwrap();
        let queue = self.queue.take().unwrap();
        let config = self.config.take().unwrap();
        let mut egui_state = self.egui_state.take().unwrap();
        let mut egui_renderer = self.egui_renderer.take().unwrap();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let surface = instance.create_surface(window.clone())?;
        surface.configure(&device, &config);

        event_loop.run(move |event, elwt| {
            elwt.set_control_flow(ControlFlow::Poll);

            match event {
                Event::WindowEvent { window_id, event } if window_id == window.id() => {
                    let response = egui_state.on_window_event(&window, &event);
                    if response.consumed {
                        return;
                    }

                    match event {
                        WindowEvent::CloseRequested => elwt.exit(),
                        WindowEvent::Resized(new_size) => {
                            let mut config = config.clone();
                            config.width = new_size.width.max(1);
                            config.height = new_size.height.max(1);
                            surface.configure(&device, &config);
                            // Store updated config back
                            drop(config);
                        }
                        WindowEvent::RedrawRequested => {
                            // Process all available messages
                            while let Ok(message) = self.app_state.message_receiver.try_recv() {
                                match message {
                                    AppMessage::TaskCompleted(result) => {
                                        self.app_state.status_message = format!("STATUS: {}", result);
                                        self.app_state.text_buffer.add_line(format!("[ASYNC] {}", result));
                                    }
                                    AppMessage::NewLine(line) => self.app_state.text_buffer.add_line(line),
                                }
                            }

                            let raw_input = egui_state.take_egui_input(&window);
                            let output = self.egui_ctx.run(raw_input, |ctx| {
                                draw_ui(ctx, &mut self.app_state, mpsc::Sender::clone(&self._message_sender));
                            });

                            egui_state.handle_platform_output(&window, output.platform_output);

                            let screen_descriptor = ScreenDescriptor {
                                size_in_pixels: [config.width, config.height],
                                pixels_per_point: window.scale_factor() as f32,
                            };
                            let paint_jobs = self.egui_ctx.tessellate(output.shapes, screen_descriptor.pixels_per_point);

                            let frame = match surface.get_current_texture() {
                                Ok(frame) => frame,
                                Err(e) => {
                                    eprintln!("Failed to get surface texture: {:?}", e);
                                    return;
                                }
                            };
                            let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
                            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

                            egui_renderer.update_buffers(&device, &queue, &mut encoder, &paint_jobs, &screen_descriptor);

                            let clear_color = wgpu::Color { r: 10.0/255.0, g: 10.0/255.0, b: 10.0/255.0, a: 1.0 };
                            {
                                let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                    label: None,
                                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                        view: &view,
                                        resolve_target: None,
                                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear_color), store: wgpu::StoreOp::Store },
                                    })],
                                    depth_stencil_attachment: None,
                                    timestamp_writes: None,
                                    occlusion_query_set: None,
                                });
                                egui_renderer.render(&mut render_pass, &paint_jobs, &screen_descriptor);
                            }
                            queue.submit(Some(encoder.finish()));
                            frame.present();
                        }
                        _ => {}
                    }
                }
                Event::AboutToWait => {
                    // Check if we have new messages to process
                    if self.app_state.message_receiver.try_recv().is_ok() {
                        window.request_redraw();
                    } else {
                        // Throttle redraws to reduce CPU usage
                        elwt.set_control_flow(ControlFlow::Wait);
                    }
                }
                _ => (),
            }
        })?;

        // Resources will be automatically cleaned up when the struct is dropped
        Ok(())
    }
}

impl Drop for NeoTermApp {
    fn drop(&mut self) {
        println!("NeoTermApp: Cleaning up resources...");
        // All resources are automatically cleaned up when Arc and Box are dropped
        // Explicit cleanup is handled by the respective libraries
    }
}

fn process_command(command: &str, state: &mut AppState, sender: mpsc::Sender<AppMessage>) {
    let cmd = command.trim().to_lowercase();

    match cmd.as_str() {
        "help" | "?" => {
            let help_text = [
                "Available commands:",
                "  help, ?          - Show this help message",
                "  clear            - Clear the terminal",
                "  status           - Show system status",
                "  echo <text>      - Echo text back",
                "  time             - Show current time",
                "  date             - Show current date",
                "  async-task       - Run async task",
                "  log              - Generate log entry",
                "  scroll-top       - Scroll to top",
                "  scroll-bottom    - Scroll to bottom",
            ];
            for line in &help_text {
                state.text_buffer.add_line(line.to_string());
            }
        }
        "clear" => {
            state.text_buffer = TextBuffer::new(1000);
            state.text_buffer.add_line("Terminal cleared.".to_string());
        }
        "status" => {
            state.text_buffer.add_line(format!("System Status: {}", state.status_message));
        }
        cmd if cmd.starts_with("echo ") => {
            let echo_text = &cmd[5..];
            state.text_buffer.add_line(echo_text.to_string());
        }
        "time" => {
            let time = chrono::Local::now().format("%H:%M:%S");
            state.text_buffer.add_line(format!("Current time: {}", time));
        }
        "date" => {
            let date = chrono::Local::now().format("%Y-%m-%d");
            state.text_buffer.add_line(format!("Current date: {}", date));
        }
        "async-task" => {
            let tx = sender.clone();
            tokio::spawn(async move {
                if tx.send(AppMessage::NewLine("[COMMAND] Async task started...".to_string())).await.is_err() {
                    eprintln!("Failed to send command response");
                    return;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
                if tx.send(AppMessage::TaskCompleted("Command executed successfully.".to_string())).await.is_err() {
                    eprintln!("Failed to send command completion");
                }
            });
            state.text_buffer.add_line("Async task initiated.".to_string());
        }
        "log" => {
            let tx = sender.clone();
            tokio::spawn(async move {
                if tx.send(AppMessage::NewLine(format!("[COMMAND] Log entry at {}", chrono::Local::now().format("%H:%M:%S")))).await.is_err() {
                    eprintln!("Failed to send log message");
                }
            });
        }
        "scroll-top" => {
            state.text_buffer.scroll_to_top();
            state.text_buffer.add_line("Scrolled to top.".to_string());
        }
        "scroll-bottom" => {
            state.text_buffer.scroll_to_bottom();
            state.text_buffer.add_line("Scrolled to bottom.".to_string());
        }
        "" => {
            // Empty command, do nothing
        }
        _ => {
            state.text_buffer.add_line(format!("Unknown command: '{}'. Type 'help' for available commands.", cmd));
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = NeoTermApp::new().await?;
    app.run()
}

fn draw_ui(ctx: &Context, state: &mut AppState, sender: mpsc::Sender<AppMessage>) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.heading("SYSTEM CONSOLE");
        ui.separator();

        let text_frame = egui::Frame::dark_canvas(ui.style());
        text_frame.show(ui, |ui| {
             ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(!state.text_buffer.is_at_bottom())
                .show(ui, |ui| {
                    ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                        for line in state.text_buffer.visible_lines() {
                            ui.label(line);
                        }
                    });
                    ui.allocate_space(ui.available_size());
                });
        });

        ui.add_space(8.0);

        ui.vertical(|ui| {
            ui.heading("ASYNC_TASK_MODULE");
            if ui.button("> EXECUTE_SLOW_TASK (2 seconds)").clicked() {
                let tx = sender.clone();
                tokio::spawn(async move {
                    if tx.send(AppMessage::NewLine("[ASYNC] Task started...".to_string())).await.is_err() {
                        eprintln!("Failed to send async task start message");
                        return;
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    if tx.send(AppMessage::TaskCompleted("Task completed successfully.".to_string())).await.is_err() {
                        eprintln!("Failed to send task completion message");
                    }
                });
            }
            if ui.button("> GENERATE LOG LINE").clicked() {
                let tx = sender.clone();
                tokio::spawn(async move {
                    if tx.send(AppMessage::NewLine(format!("[LOG] Sample log entry at {}", chrono::Local::now().format("%H:%M:%S")))).await.is_err() {
                        eprintln!("Failed to send log message");
                    }
                });
            }
        });
        ui.add_space(8.0);

        // Command input section
        ui.horizontal(|ui| {
            ui.heading("COMMAND INPUT");
            ui.add_space(8.0);
        });

        ui.horizontal(|ui| {
            ui.label(">");
            let response = ui.text_edit_singleline(&mut state.command_input);
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                if !state.command_input.trim().is_empty() {
                    let command = state.command_input.clone();
                    state.text_buffer.add_line(format!("> {}", command));
                    process_command(&command, state, sender.clone());
                    state.command_input.clear();
                }
            }
            if ui.button("Execute").clicked() {
                if !state.command_input.trim().is_empty() {
                    let command = state.command_input.clone();
                    state.text_buffer.add_line(format!("> {}", command));
                    process_command(&command, state, sender.clone());
                    state.command_input.clear();
                }
            }
        });

        // Scroll controls
        ui.horizontal(|ui| {
            ui.heading("SCROLL CONTROLS");
            if ui.button("↑ Top").clicked() {
                state.text_buffer.scroll_to_top();
            }
            if ui.button("↑ Up").clicked() {
                state.text_buffer.scroll_up();
            }
            if ui.button("↓ Down").clicked() {
                state.text_buffer.scroll_down();
            }
            if ui.button("↓ Bottom").clicked() {
                state.text_buffer.scroll_to_bottom();
            }

            let total_lines = state.text_buffer.lines.len();
            let visible_lines = state.text_buffer.max_lines;
            let scroll_pos = state.text_buffer.scroll_position;

            if total_lines > visible_lines {
                let percentage = if total_lines > 0 {
                    ((scroll_pos as f32) / (total_lines - visible_lines) as f32 * 100.0) as usize
                } else {
                    0
                };
                ui.label(format!("Position: {}% ({}/{})",
                    percentage, scroll_pos + 1, total_lines));
            } else {
                ui.label(format!("Lines: {}/{}", total_lines, visible_lines));
            }
        });

        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            ui.separator();
            ui.label(&state.status_message);
        });
    });
}