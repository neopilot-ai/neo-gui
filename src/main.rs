use egui::Key;
use egui_wgpu::ScreenDescriptor;
use egui_winit::State as EguiWinitState;
use std::iter;
use tokio::sync::mpsc;
use winit::{
    event::{Event, WindowEvent},
    event_loop::{EventLoopBuilder, EventLoopProxy},
    window::Window,
};

// NEW: Define messages that can be sent from background tasks to the UI
#[derive(Debug)]
enum AppMessage {
    // NEW: A message to add a new line to our text buffer
    NewLine(String),
    // A message to indicate a task has completed with a result
    TaskCompleted(String),
}

// NEW: A function to create our custom "Hacker Theme".
// This returns a complete egui::Style object.
fn create_hacker_theme() -> egui::Style {
    let mut style = egui::Style::default();

    // Use egui's built-in dark theme as a base.
    style.visuals = egui::Visuals::dark();

    // Define our custom colors
    let hacker_green = egui::Color32::from_rgb(0, 255, 68); // A bright, vibrant green
    let background_dark = egui::Color32::from_rgb(10, 10, 10); // A very dark gray
    let mid_gray = egui::Color32::from_rgb(60, 60, 60);
    let light_gray = egui::Color32::from_rgb(100, 100, 100);

    // --- Global Visuals ---
    style.visuals.window_fill = background_dark;
    style.visuals.panel_fill = background_dark;
    style.visuals.window_stroke = egui::Stroke::new(1.0, mid_gray);
    style.visuals.selection.bg_fill = hacker_green.linear_multiply(0.3); // Selection background
    style.visuals.selection.stroke = egui::Stroke::NONE;

    // --- Widget Visuals ---
    let widgets = &mut style.visuals.widgets;
    widgets.noninteractive.bg_fill = background_dark;
    widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, light_gray); // Default text color

    // Interactive widgets (buttons, text boxes)
    widgets.inactive.bg_fill = mid_gray.linear_multiply(0.5);
    widgets.inactive.fg_stroke = egui::Stroke::new(1.0, hacker_green);
    
    widgets.hovered.bg_fill = mid_gray;
    widgets.hovered.fg_stroke = egui::Stroke::new(1.5, hacker_green); // Make text bold on hover
    
    widgets.active.bg_fill = light_gray;
    widgets.active.fg_stroke = egui::Stroke::new(2.0, hacker_green);

    // --- Headings and special text ---
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(24.0, egui::FontFamily::Monospace),
    );
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::new(16.0, egui::FontFamily::Monospace),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(16.0, egui::FontFamily::Monospace),
    );
    
    style
}

// NEW: A struct to manage our text buffer state
struct TextBuffer {
    lines: Vec<String>,
    // A simple way to limit memory usage
    max_lines: usize,
}

impl TextBuffer {
    fn new(max_lines: usize) -> Self {
        Self {
            lines: Vec::new(),
            max_lines,
        }
    }

    fn add_line(&mut self, line: String) {
        self.lines.push(line);
        if self.lines.len() > self.max_lines {
            // Remove old lines to prevent infinite memory growth
            self.lines.remove(0);
        }
    }
}

struct AppState<'window> {
    surface: wgpu::Surface<'window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: winit::dpi::PhysicalSize<u32>,
    window: &'window Window,
    egui_ctx: egui::Context,
    egui_state: EguiWinitState,
    egui_renderer: egui_wgpu::Renderer,
    frame_count: usize,
    sender: mpsc::Sender<AppMessage>,
    receiver: mpsc::Receiver<AppMessage>,
    
    // NEW: Add the text buffer to our application state
    text_buffer: TextBuffer,
    // NEW: A string to hold the user's current input
    input_buffer: String,
}

impl<'window> AppState<'window> {
async fn new(window: &'window Window, _event_loop_proxy: EventLoopProxy<AppMessage>) -> Self {
        let size = window.inner_size();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance.create_surface(window).unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("Main Device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                },
                None, // Trace path
            )
            .await
            .unwrap();

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);
        
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: surface_caps.present_modes[0], // Vsync
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let egui_ctx = egui::Context::default();
        
        // MODIFIED: Apply our custom theme right after creating the context.
        egui_ctx.set_style(create_hacker_theme());

        let egui_state = EguiWinitState::new(
            egui_ctx.clone(),
            egui::ViewportId::default(),
            &window,
            None,
            None
        );
        let egui_renderer = egui_wgpu::Renderer::new(&device, config.format, None, 1);

        // NEW: Create the channel for async communication
        let (sender, receiver) = mpsc::channel(100); // Increased channel buffer size

        // NEW: Initialize the text buffer
        let mut text_buffer = TextBuffer::new(1000); // Keep up to 1000 lines of history
        text_buffer.add_line("SYSTEM BOOT COMPLETE.".into());
        text_buffer.add_line("Awaiting command...".into());

        Self {
            window,
            surface,
            device,
            queue,
            config,
            size,
            egui_ctx,
            egui_state,
            egui_renderer,
            frame_count: 0,
            sender,
            receiver,
            text_buffer,
            // NEW: Initialize the input buffer
            input_buffer: String::new(),
        }
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    pub fn handle_event(&mut self, event: &WindowEvent) -> bool {
        self.egui_state.on_window_event(self.window, event).consumed
    }


    // MODIFIED: Handle the new message type
    pub fn handle_message(&mut self, message: AppMessage) {
        match message {
            AppMessage::TaskCompleted(result) => {
                self.text_buffer.add_line(format!("[OK] Task finished: {}", result));
            }
            AppMessage::NewLine(line) => {
                self.text_buffer.add_line(line);
            }
        }
    }

    pub fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        // NEW: Poll the receiver for messages from our tasks without blocking.
        // If a message is ready, we immediately process it.
        while let Ok(message) = self.receiver.try_recv() {
            self.handle_message(message);
        }
        let output_frame = self.surface.get_current_texture()?;
        let view = output_frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });

        let raw_input = self.egui_state.take_egui_input(&self.window);
        
        // Clone the sender so we can move it into the UI closure
        let sender = self.sender.clone();

        // Pass a mutable reference to the text buffer into the UI closure
        let text_buffer = &mut self.text_buffer;
        
        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            // We'll use a TopBottomPanel to dock the input field to the bottom.
            egui::TopBottomPanel::bottom("input_panel")
                .resizable(false)
                .min_height(30.0)
                .show(ctx, |ui| {
                    // This is the core implementation of the prompt.
                    ui.horizontal(|ui| {
                        ui.label(">");
                        
                        // 1. Create the TextEdit widget
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.input_buffer)
                                .desired_width(f32::INFINITY) // Take up all available horizontal space
                                .lock_focus(true) // Keep focus on the input field
                        );

                        // 2. Check if the user pressed Enter
                        if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            // 3. Send the command to be processed
                            let command = self.input_buffer.trim().to_string();
                            if !command.is_empty() {
                                // Echo the command to the main buffer
                                let _ = sender.try_send(AppMessage::NewLine(format!("> {}", command)));
                                
                                // Parse and execute the command
                                if command == "run_task" {
                                    // Clone the sender to move into the async task
                                    let task_sender = sender.clone();
                                    
                                    // Spawn the background task
                                    tokio::spawn(async move {
                                        // Send initial status
                                        let _ = task_sender.try_send(AppMessage::NewLine("[INFO] Starting background task...".to_string()));
                                        
                                        // Simulate some work with a 2-second delay
                                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                                        
                                        // Send completion message
                                        let _ = task_sender.try_send(AppMessage::TaskCompleted("Background task executed successfully".to_string()));
                                    });
                                } else if !command.is_empty() {
                                    // Handle other commands
                                    let _ = sender.try_send(AppMessage::NewLine(format!("[ERROR] Unknown command: {}", command)));
                                }
                            }
                            
                            // 4. Clear the input buffer for the next command
                            self.input_buffer.clear();
                            
                            // 5. Regain focus for the next command
                            response.request_focus();
                        }
                    });
                });
            
            // The central panel now contains our scrollable text buffer
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("SYSTEM CONSOLE");
                
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in &text_buffer.lines {
                            ui.label(line);
                        }
                    });
            });
        });
        
        self.frame_count += 1;
        self.egui_state.handle_platform_output(self.window, full_output.platform_output);
        let tris = self.egui_ctx.tessellate(full_output.shapes, full_output.pixels_per_point);

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: self.window.scale_factor() as f32,
        };

        for (id, image_delta) in &full_output.textures_delta.set {
            self.egui_renderer.update_texture(&self.device, &self.queue, *id, image_delta);
        }
        self.egui_renderer.update_buffers(&self.device, &self.queue, &mut encoder, &tris, &screen_descriptor);

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Egui Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // MODIFIED: Change the wgpu clear color to match our theme's background
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 10.0/255.0, g: 10.0/255.0, b: 10.0/255.0, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            self.egui_renderer.render(&mut render_pass, &tris, &screen_descriptor);
        }

        for id in &full_output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }

        self.queue.submit(iter::once(encoder.finish()));
        
        output_frame.present();

        Ok(())
    }
}

// NEW: Tag the main function with tokio's macro to set up the runtime
#[tokio::main]
async fn main() {
    env_logger::init();

    // NEW: Create an event loop that can handle custom events (our AppMessage)
    let event_loop = EventLoopBuilder::<AppMessage>::with_user_event().build().unwrap();
    let window = Window::new(&event_loop).unwrap();
    window.set_title("Neo-Term"); // MODIFIED: New title!

    // NEW: Create a proxy to send custom events to the event loop from other threads
    let event_loop_proxy = event_loop.create_proxy();

    // Pass the proxy to our app state. This is not strictly needed for the mpsc channel
    // approach but is good practice for winit user events.
    let mut state = AppState::new(&window, event_loop_proxy).await;

    event_loop.run(move |event, elwt| {
        match event {
            Event::WindowEvent { window_id, event } if window_id == state.window.id() => {
                if !state.handle_event(&event) {
                    match event {
                        WindowEvent::CloseRequested => elwt.exit(),
                        WindowEvent::Resized(physical_size) => state.resize(physical_size),
                        WindowEvent::ScaleFactorChanged { .. } => state.resize(state.window.inner_size()),
                        _ => {}
                    }
                }
            }
            Event::AboutToWait => {
                state.window.request_redraw();
            }
            // NEW: Handle the custom events sent from our background tasks
            Event::UserEvent(message) => {
                state.handle_message(message);
            }
            Event::WindowEvent { event: WindowEvent::RedrawRequested, .. } => {
                match state.render() {
                    Ok(_) => {}
                    Err(wgpu::SurfaceError::Lost) => state.resize(state.size),
                    Err(wgpu::SurfaceError::OutOfMemory) => elwt.exit(),
                    Err(e) => eprintln!("{:?}", e),
                }
            }
            _ => {}
        }
    }).unwrap();
}

