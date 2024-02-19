#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui::*;
use image::*;
use image_cleanup::ImageCleaner;
use tokio;
use tokio::task::JoinHandle;

#[tokio::main]
async fn main() -> Result<(), eframe::Error> {
    env_logger::init(); // Log to stderr (if you run with `RUST_LOG=debug`).
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_maximized(true)
            .with_icon(Arc::new(icon_image())),
        ..Default::default()
    };

    eframe::run_native(
        "Image Cleanup",
        options,
        Box::new(|cc| Box::new(ImageCleanup::new(&cc.egui_ctx))),
    )
}

struct ImageCleanup {
    cleaner: ImageCleaner,
    original_preview_image: DynamicImage,
    preview_image_handle: TextureHandle,
    processing_preview_image: Arc<Mutex<Option<DynamicImage>>>,
    image_paths: Vec<PathBuf>,
    export_progess: Arc<Mutex<f32>>,
    export_task: Option<JoinHandle<()>>,

    // Preview settings
    preview_page: u16,
    preview_speck_fill_color: [u8; 3],
    preview_off_white_fill_color: [u8; 3],
    preview_zoom: f32,
    preview_zoom_speed: f32,
    preview_min_zoom: f32,
    preview_max_zoom: f32,
    preview_offset: Pos2,   // In UI pixels
    preview_velocity: Vec2, // In UI pixels
    previous_rect: Rect,
}

fn dynamic_image_to_color_image(image: &DynamicImage) -> ColorImage {
    let size = [image.width() as _, image.height() as _];
    let image_buffer = image.to_rgba8();
    let pixels = image_buffer.as_flat_samples();
    ColorImage::from_rgba_unmultiplied(size, pixels.as_slice())
}

fn dynamic_image_to_handle(
    ctx: &Context,
    name: impl Into<String>,
    image: &DynamicImage,
) -> TextureHandle {
    let image = dynamic_image_to_color_image(image);
    ctx.load_texture(
        name,
        image,
        TextureOptions {
            magnification: TextureFilter::Nearest,
            minification: TextureFilter::Linear,
            wrap_mode: TextureWrapMode::ClampToEdge,
        },
    )
}

fn demo_image() -> DynamicImage {
    image::load_from_memory_with_format(include_bytes!("../assets/demo_page.png"), ImageFormat::Png)
        .unwrap()
}

fn icon_image() -> IconData {
    let image =
        image::load_from_memory_with_format(include_bytes!("../assets/icon.png"), ImageFormat::Png)
            .unwrap();

    IconData {
        width: image.width(),
        height: image.height(),
        rgba: image.as_bytes().into(),
    }
}

impl ImageCleanup {
    fn new(ctx: &Context) -> Self {
        let original_preview_image = demo_image();
        let preview_image_handle =
            dynamic_image_to_handle(ctx, "preview_image", &original_preview_image);

        let mut s = Self {
            cleaner: ImageCleaner::default(),
            preview_page: 1,
            processing_preview_image: Arc::new(Mutex::new(None)),
            preview_image_handle,
            image_paths: Vec::new(),
            original_preview_image,
            export_progess: Arc::new(Mutex::new(0.0)),
            export_task: None,
            preview_speck_fill_color: [255, 0, 255],
            preview_off_white_fill_color: [255, 255, 255],
            preview_zoom: -0.025,
            preview_min_zoom: -1.0,
            preview_max_zoom: 8.0,
            preview_zoom_speed: 0.0025,
            preview_offset: Pos2::ZERO,
            preview_velocity: Vec2::ZERO,
            previous_rect: ctx.screen_rect(),
        };

        s.clean_preview();
        s
    }

    fn on_images_update(&mut self, paths: Vec<PathBuf>) {
        self.image_paths = paths;
        self.new_preview_image();
    }

    fn new_preview_image(&mut self) {
        self.original_preview_image = if !self.image_paths.is_empty() {
            image::io::Reader::open(&self.image_paths[(self.preview_page - 1) as usize])
                .unwrap()
                .decode()
                .unwrap()
        } else {
            demo_image()
        };

        self.clean_preview();
    }

    fn clean_preview(&mut self) {
        let cleaner = ImageCleaner {
            speck_fill_color: self.preview_speck_fill_color,
            off_white_fill_color: self.preview_off_white_fill_color,
            ..self.cleaner
        };
        let original_preview_image = self.original_preview_image.clone();
        let handle = self.processing_preview_image.clone();
        tokio::spawn(async move {
            let clean_preview_image = cleaner.clean(&original_preview_image);
            let mut handle = handle.lock().unwrap();
            *handle = Some(clean_preview_image);
        });
    }

    async fn export_all(
        image_paths: Vec<PathBuf>,
        cleaner: ImageCleaner,
        progress: Arc<Mutex<f32>>,
    ) {
        *progress.lock().unwrap() = 0.0;

        for (i, path) in image_paths.iter().enumerate() {
            tokio::task::yield_now().await;
            *progress.lock().unwrap() = (i + 1) as f32 / image_paths.len() as f32;
            let image = image::io::Reader::open(path).unwrap().decode().unwrap();
            let cleaned_image = cleaner.clean(&image);
            cleaned_image.save(path).unwrap();
        }
    }
}

impl eframe::App for ImageCleanup {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        SidePanel::right("parameters").resizable(false).show(ctx, |ui| {
            Grid::new("parameters")
                .spacing([40.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    if ui.button("Open images…").clicked() {
						let extensions: Vec<&str> = [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::Tiff, ImageFormat::WebP].into_iter().flat_map(|f| f.extensions_str().iter().copied()).collect();
                        if let Some(paths) = rfd::FileDialog::new().add_filter("Image files", extensions.as_slice()).pick_files() {
                            self.on_images_update(paths);
                        }
                    }
                    ui.end_row();

                    ui.label("Off-white threshold")
                        .on_hover_text("Colors with their r, g, and b values greater than this are considered off-white, and will be filled");
                    if ui.add(Slider::new(&mut self.cleaner.off_white_threshold, 0..=255)).changed() {
                        self.clean_preview();
                    }
                    ui.end_row();

                    ui.label("Speck size threshold")
                        .on_hover_text("Clusters that have an area smaller than this will be filled");
                    if ui.add(Slider::new(&mut self.cleaner.speck_size_threshold, 0..=60).clamp_to_range(false).suffix("px²")).changed() {
                        self.clean_preview();
                    }
                    ui.end_row();

                    ui.label("Speck lightness threshold")
                        .on_hover_text("Clusters that have an average rgb value greater than this will be filled");
                    if ui.add(Slider::new(&mut self.cleaner.speck_lightness_threshold, 0..=255)).changed() {
                        self.clean_preview();
                    }
                    ui.end_row();


                    ui.label("Speck margins")
                        .on_hover_text("Clusters that are within these margins will be filled");
                    ui.end_row();

                    ui.label("\t- x");
                    if ui.add(Slider::new(&mut self.cleaner.speck_margins.0, 0..=100).clamp_to_range(false).suffix("px")).changed() {
                        self.clean_preview();
                    }
                    ui.end_row();

                    ui.label("\t- y");
                    if ui.add(Slider::new(&mut self.cleaner.speck_margins.1, 0..=100).clamp_to_range(false).suffix("px")).changed() {
                        self.clean_preview();
                    }
                    ui.end_row();


                    ui.label("Isolation thresholds")
                        .on_hover_text(" (Clusters that have an area smaller than this and aren't within this distance of another cluster that is will be filled");
                    ui.end_row();

                    ui.label("\t- size");
                    if ui.add(Slider::new(&mut self.cleaner.isolation_size_threshold, 0..=150).clamp_to_range(false).suffix("px²")).changed() {
                        self.clean_preview();
                    }
                    ui.end_row();
                    ui.label("\t- distance");
                    if ui.add(Slider::new(&mut self.cleaner.isolation_distance_threshold, 0..=200).clamp_to_range(false).suffix("px")).changed() {
                        self.clean_preview();
                    }
                    ui.end_row();

                    ui.label("Speck fill color")
                        .on_hover_text("What color to fill in specks, useful for verification");
                    if ui.color_edit_button_srgb(&mut self.cleaner.speck_fill_color).changed() {
                        self.clean_preview();
                    }
                    ui.end_row();

                    ui.label("Off-white fill color")
                        .on_hover_text("What color to fill in off-white pixels, useful for verification");
                    if ui.color_edit_button_srgb(&mut self.cleaner.off_white_fill_color).changed() {
                        self.clean_preview();
                    }
                    ui.end_row();

					if ui.add_enabled(!self.image_paths.is_empty() && self.export_task.is_none(), Button::new("Export all")).on_disabled_hover_text("No images have been opened or they are currently exporting").clicked() {
                        self.export_task = Some(tokio::spawn(Self::export_all(self.image_paths.clone(), self.cleaner, self.export_progess.clone())));
					}


                    if let Some(task) = &self.export_task {
                        if task.is_finished() {
                            self.export_task = None;
                        } else {
                            Window::new("Exporting...").show(ctx, |ui| {
                                ui.add(ProgressBar::new(*self.export_progess.lock().unwrap()).show_percentage());
                                ctx.request_repaint();

                                if ui.button("Cancel").clicked() {
                                    task.abort();
                                }
                            });
                        }
                    }



                    ui.end_row();


                });
        });

        SidePanel::left("preview_tools")
            .resizable(false)
            .show(ctx, |ui| {
                Grid::new("preview_parameters")
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label("Preview Page");
                        if ui
                            .add(
                                DragValue::new(&mut self.preview_page)
                                    .update_while_editing(false)
                                    .speed(1.0 / 72.0)
                                    .clamp_range(1..=self.image_paths.len().at_least(1)),
                            )
                            .changed()
                        {
                            self.new_preview_image();
                        }
                        ui.end_row();

                        ui.label("Preview speck fill color");
                        if ui
                            .color_edit_button_srgb(&mut self.preview_speck_fill_color)
                            .changed()
                        {
                            self.clean_preview();
                        }
                        ui.end_row();

                        ui.label("Preview off-white fill color");
                        if ui
                            .color_edit_button_srgb(&mut self.preview_off_white_fill_color)
                            .changed()
                        {
                            self.clean_preview();
                        }
                        ui.end_row();

                        ui.label("Zoom");
                        ui.add(
                            Slider::new(
                                &mut self.preview_zoom,
                                self.preview_min_zoom..=self.preview_max_zoom,
                            )
                            .step_by(0.01),
                        );
                        ui.end_row();
                    });
            });

        CentralPanel::default()
            .frame(eframe::egui::Frame::none().fill(Color32::from_rgb(27, 26, 31)))
            .show(ctx, |ui| {
                if ui.max_rect().area() < 1.0 {
                    // Prevents a crash
                    return;
                }

                ui.set_clip_rect(ui.max_rect());

                if let Ok(mut mutex) = self.processing_preview_image.try_lock() {
                    if let Some(preview_image) = &*mutex {
                        self.preview_image_handle =
                            dynamic_image_to_handle(ctx, "preview_image", &preview_image);
                        *mutex = None;
                    }
                }

                let scroll_delta = ctx.input(|i| i.smooth_scroll_delta.y);
                self.preview_zoom = (self.preview_zoom + scroll_delta * self.preview_zoom_speed)
                    .max(self.preview_min_zoom)
                    .min(self.preview_max_zoom);
                let zoom = 2f32.powf(self.preview_zoom);

                if ctx.input(|i| i.raw_scroll_delta.y) != 0.0 {
                    self.preview_velocity = Vec2::ZERO;
                }

                // Drag to scroll
                let content_response = ui.interact(ui.max_rect(), ui.id(), Sense::drag());
                if content_response.dragged() {
                    ui.input(|input| {
                        self.preview_offset += input.pointer.delta();
                        self.preview_velocity = input.pointer.velocity();
                    });
                } else {
                    // Kinetic scrolling
                    let stop_speed = 20.0; // Pixels per second.
                    let friction_coeff = 1000.0; // Pixels per second squared.
                    let dt = ui.input(|i| i.unstable_dt);

                    let friction = friction_coeff * dt;
                    if friction > self.preview_velocity.length()
                        || self.preview_velocity.length() < stop_speed
                    {
                        self.preview_velocity = Vec2::ZERO;
                    } else {
                        self.preview_velocity -= friction * self.preview_velocity.normalized();
                        // Offset has an inverted coordinate system compared to
                        // the velocity, so we subtract it instead of adding it
                        self.preview_offset += self.preview_velocity * dt;
                        ctx.request_repaint();
                    }
                }

                let dimensions = Vec2::new(
                    self.original_preview_image.width() as f32,
                    self.original_preview_image.height() as f32,
                );

                let largest_dimension =
                    (dimensions.x / ui.available_width()).max(dimensions.y / ui.available_height());
                let image_to_ui_scale = largest_dimension / zoom;
                let to_ui_pixels = |v: Vec2| v / image_to_ui_scale;
                let to_image_pixels = |v: Vec2| v * image_to_ui_scale;

                let previous_rect = self.previous_rect;
                let mouse_pos =
                    ctx.input(|i| i.pointer.latest_pos().unwrap_or(previous_rect.center()));
                let mouse_pos = previous_rect.clamp(mouse_pos);
                let mouse_pos = Vec2::new(
                    mouse_pos.x - previous_rect.left(),
                    mouse_pos.y - previous_rect.top(),
                ) / previous_rect.size();
                let mouse_pos = Vec2::new(mouse_pos.x * 2.0 - 1.0, mouse_pos.y * 2.0 - 1.0);

                let size = to_ui_pixels(dimensions);
                self.preview_offset -= ((size - previous_rect.size()) / 2.0) * mouse_pos;
                self.preview_offset = Rect::from_center_size(Pos2::ZERO, to_ui_pixels(dimensions))
                    .clamp(self.preview_offset);

                let painter = ui.painter();
                let rect = Rect::from_center_size(
                    ui.max_rect().center() + (self.preview_offset.to_vec2()),
                    size,
                );
                self.previous_rect = rect;
                painter.image(
                    self.preview_image_handle.id(),
                    rect,
                    Rect::from_x_y_ranges(0.0..=1.0, 0.0..=1.0),
                    Color32::WHITE,
                );
            });
    }
}
