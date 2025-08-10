// src/main.rs

use egui::{
    epaint::FontId, style::Spacing, Color32, Context, FontFamily, Rounding, ScrollArea, Stroke,
    Style, TextStyle, Vec2, Visuals,
};
use egui_wgpu::{Renderer, ScreenDescriptor};
use egui_winit::State;
use egui::ViewportId;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::Window,
};

/// Message enum for communication between async tasks and the UI thread.
#[derive(Debug)]
enum AppMessage {
    TaskCompleted(String),
    NewLine(String),
}

/// Manages the terminal's text content.
struct TextBuffer {
    lines: Vec<String>,
    max_lines: usize,
}

impl TextBuffer {
    fn new(max_lines: usize) -> Self {
        Self {
            lines: Vec::with_capacity(max_lines),
            max_lines,
        }
    }

    fn add_line(&mut self, line: String) {
        if self.lines.len() >= self.max_lines {
            self.lines.remove(0);
        }
        self.lines.push(line);
    }
}

/// Holds the entire application state.
struct AppState {
    text_buffer: TextBuffer,
    status_message: String,
    message_receiver: mpsc::Receiver<AppMessage>,
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

#[tokio::main]
async fn main() {
    env_logger::init();
    let event_loop = EventLoop::new().unwrap();
    let window = Arc::new(Window::new(&event_loop).unwrap());
    window.set_title("Neo-Term");

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
    let surface = instance.create_surface(window.clone()).unwrap();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
        .unwrap();

    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor::default(), None)
        .await
        .unwrap();

    let size = window.inner_size();
    let surface_caps = surface.get_capabilities(&adapter);
    let surface_format = surface_caps.formats[0];

    let mut config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: surface_format,
        width: size.width,
        height: size.height,
        present_mode: wgpu::PresentMode::Fifo,
        alpha_mode: surface_caps.alpha_modes[0],
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };
    surface.configure(&device, &config);

    // FIX: API for egui-winit 0.27 takes the event loop target.
    let egui_ctx = Context::default();
    let mut egui_state = State::new(egui_ctx.clone(), ViewportId::ROOT, &event_loop, None, None);
    let mut egui_renderer = Renderer::new(&device, surface_format, None, 1);
    egui_ctx.set_style(create_hacker_theme());

    let (message_sender, message_receiver) = mpsc::channel::<AppMessage>(100);

    let mut app_state = AppState {
        text_buffer: TextBuffer::new(1000),
        status_message: "STATUS: System nominal.".to_string(),
        message_receiver,
    };
    // Initialize with ASCII art
    app_state.text_buffer.add_line("███╗   ██╗███████╗ ██████╗".into());
    app_state.text_buffer.add_line("████╗  ██║██╔════╝██╔═══██╗".into());
    app_state.text_buffer.add_line("██╔██╗ ██║█████╗  ██║   ██║".into());
    app_state.text_buffer.add_line("██║╚██╗██║██╔══╝  ██║   ██║".into());
    app_state.text_buffer.add_line("██║ ╚████║███████╗╚██████╔╝".into());
    app_state.text_buffer.add_line("╚═╝  ╚═══╝╚══════╝ ╚═════╝ ".into());
    app_state.text_buffer.add_line("".into());
    app_state.text_buffer.add_line("Welcome to Neo-Term. Standby for commands.".into());


    event_loop.run(move |event, elwt| {
        elwt.set_control_flow(ControlFlow::Poll);

        match event {
            Event::WindowEvent { window_id, event } if window_id == window.id() => {
                // FIX: API for egui-winit 0.27 takes the context.
                let response = egui_state.on_window_event(&window, &event);
                if response.consumed {
                    return;
                }
                
                match event {
                    WindowEvent::CloseRequested => elwt.exit(),
                    WindowEvent::Resized(new_size) => {
                        config.width = new_size.width.max(1);
                        config.height = new_size.height.max(1);
                        surface.configure(&device, &config);
                    }
                    WindowEvent::RedrawRequested => {
                        // All drawing logic moved here.
                        while let Ok(message) = app_state.message_receiver.try_recv() {
                           match message {
                                AppMessage::TaskCompleted(result) => {
                                    app_state.status_message = format!("STATUS: {}", result);
                                    app_state.text_buffer.add_line(format!("[ASYNC] {}", result));
                                }
                                AppMessage::NewLine(line) => app_state.text_buffer.add_line(line),
                            }
                        }

                        let raw_input = egui_state.take_egui_input(&window);
                        let output = egui_ctx.run(raw_input, |ctx| {
                            draw_ui(ctx, &mut app_state, message_sender.clone());
                        });

                        egui_state.handle_platform_output(&window, output.platform_output);

                        let screen_descriptor = ScreenDescriptor {
                            size_in_pixels: [config.width, config.height],
                            pixels_per_point: window.scale_factor() as f32,
                        };
                        let paint_jobs = egui_ctx.tessellate(output.shapes, screen_descriptor.pixels_per_point);

                        let frame = surface.get_current_texture().expect("Failed to get surface texture");
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
                window.request_redraw();
            }
            _ => (),
        }
    }).unwrap();
}

fn draw_ui(ctx: &Context, state: &mut AppState, sender: mpsc::Sender<AppMessage>) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.heading("SYSTEM CONSOLE");
        ui.separator();
        
        let text_frame = egui::Frame::dark_canvas(ui.style());
        text_frame.show(ui, |ui| {
             ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                        for line in &state.text_buffer.lines {
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
                    tx.send(AppMessage::NewLine("[ASYNC] Task started...".to_string())).await.ok();
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    tx.send(AppMessage::TaskCompleted("Task completed successfully.".to_string())).await.ok();
                });
            }
            if ui.button("> GENERATE LOG LINE").clicked() {
                let tx = sender.clone();
                tokio::spawn(async move {
                    tx.send(AppMessage::NewLine(format!("[LOG] Sample log entry at {}", chrono::Local::now().format("%H:%M:%S")))).await.ok();
                });
            }
        });

        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            ui.separator();
            ui.label(&state.status_message);
        });
    });
}