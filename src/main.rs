//#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui::*;
use image::*;
use image_cleanup::*;
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
    analyzer: ImageAnalyzer,
    cleaner: ImageCleaner,

    preview_image_width: u32,
    preview_image_height: u32,
    analyzed_preview_image: Arc<Mutex<AnalyzedImage>>,
    cleaned_preview_image: Arc<Mutex<RgbImage>>,
    preview_image_handle: TextureHandle,

    image_paths: Vec<PathBuf>,

    analyze_preview_task: Option<JoinHandle<()>>,
    clean_preview_task: Option<JoinHandle<()>>,
    export_task: Option<JoinHandle<()>>,
    export_progess: Arc<Mutex<f32>>,

    // Preview settings
    previews_needs_analyze: bool,
    previews_needs_clean: bool,
    preview_page: u16,
    preview_speck_fill_color: [u8; 3],
    preview_background_fill_color: [u8; 3],
    preview_zoom: f32,
    preview_zoom_speed: f32,
    preview_min_zoom: f32,
    preview_max_zoom: f32,
    preview_offset: Vec2,   // In image pixels
    preview_velocity: Vec2, // In image pixels
    preview_margin_color: Color32,
}

fn rgb_image_to_color_image(image: &RgbImage) -> ColorImage {
    let size = [image.width() as _, image.height() as _];
    let pixels = image.as_flat_samples();
    ColorImage::from_rgb(size, pixels.as_slice())
}

fn rgb_image_to_handle(ctx: &Context, name: impl Into<String>, image: &RgbImage) -> TextureHandle {
    let image = rgb_image_to_color_image(image);
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

fn demo_image() -> RgbImage {
    image::load_from_memory_with_format(include_bytes!("../assets/demo_page.png"), ImageFormat::Png)
        .unwrap()
        .to_rgb8()
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
        let analyzer = ImageAnalyzer::default();
        let analyzed_image = analyzer.analyze(&original_preview_image);
        let cleaner = ImageCleaner::default();

        let preview_speck_fill_color = [255, 0, 255];
        let preview_background_fill_color = [255, 255, 255];
        let preview_cleaner = ImageCleaner {
            speck_fill_color: preview_speck_fill_color,
            background_fill_color: preview_background_fill_color,
            ..cleaner
        };
        let cleaned_image = preview_cleaner.clean(&analyzed_image);
        let preview_image_handle = rgb_image_to_handle(ctx, "preview_image", &cleaned_image);

        Self {
            analyzer,
            cleaner,
            preview_page: 1,
            analyzed_preview_image: Arc::new(Mutex::new(analyzed_image)),
            cleaned_preview_image: Arc::new(Mutex::new(cleaned_image)),
            preview_image_handle,
            image_paths: Vec::new(),
            analyze_preview_task: None,
            clean_preview_task: None,
            export_task: None,
            export_progess: Arc::new(Mutex::new(0.0)),
            previews_needs_analyze: false,
            previews_needs_clean: false,
            preview_speck_fill_color,
            preview_background_fill_color,
            preview_zoom: 0.0,
            preview_min_zoom: -1.0,
            preview_max_zoom: 8.0,
            preview_zoom_speed: 0.0025,
            preview_offset: Vec2::ZERO,
            preview_velocity: Vec2::ZERO,
            preview_margin_color: Color32::from_rgba_unmultiplied(0, 0, 255, 128),
            preview_image_width: original_preview_image.width(),
            preview_image_height: original_preview_image.height(),
        }
    }

    fn on_images_update(&mut self, paths: Vec<PathBuf>) {
        self.image_paths = paths;
        self.new_preview_image();
    }

    fn new_preview_image(&mut self) {
        self.queue_analyze_preview();
    }

    fn queue_analyze_preview(&mut self) {
        self.previews_needs_analyze = true;
    }

    fn queue_clean_preview(&mut self) {
        self.previews_needs_clean = true;
    }

    async fn export_all(
        image_paths: Vec<PathBuf>,
        analyzer: ImageAnalyzer,
        cleaner: ImageCleaner,
        progress: Arc<Mutex<f32>>,
    ) {
        *progress.lock().unwrap() = 0.0;

        for (i, path) in image_paths.iter().enumerate() {
            tokio::task::yield_now().await;
            *progress.lock().unwrap() = (i + 1) as f32 / image_paths.len() as f32;
            let image = image::io::Reader::open(path)
                .unwrap()
                .decode()
                .unwrap()
                .to_rgb8();
            let analyzed_image = analyzer.analyze(&image);
            let cleaned_image = cleaner.clean(&analyzed_image);
            cleaned_image.save(path).unwrap();
        }
    }
}

impl eframe::App for ImageCleanup {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Here's how it works:
        // When the preview image is changed, it gets analyzed.
        if let Some(analyze_task) = &self.analyze_preview_task {
            if analyze_task.is_finished() {
                self.analyze_preview_task = None;
                // Then the program is told to clean the preview, using the new AnalyzedImage.
                // (It's also told to clean every time the user makes changes to the cleaner parameters)
                self.queue_clean_preview();
            }
        }

        let is_analyzing = self.analyze_preview_task.is_some();
        if self.previews_needs_analyze && !is_analyzing {
            self.previews_needs_analyze = false;

            let original_preview_image = if !self.image_paths.is_empty() {
                image::io::Reader::open(&self.image_paths[(self.preview_page - 1) as usize])
                    .unwrap()
                    .decode()
                    .unwrap()
                    .to_rgb8()
            } else {
                demo_image()
            };

            self.preview_image_width = original_preview_image.width();
            self.preview_image_height = original_preview_image.height();

            let analyzer = self.analyzer;
            let analyzed_handle = self.analyzed_preview_image.clone();
            self.analyze_preview_task = Some(tokio::spawn(async move {
                let analyzed = analyzer.analyze(&original_preview_image);
                *analyzed_handle.lock().unwrap() = analyzed;
            }));
        }

        if let Some(clean_task) = &self.clean_preview_task {
            if clean_task.is_finished() {
                self.clean_preview_task = None;
                // Then once it's done cleaning, update the user's preview.
                self.preview_image_handle = rgb_image_to_handle(
                    ctx,
                    "preview_image",
                    &*self.cleaned_preview_image.lock().unwrap(),
                );
            }
        }

        let is_cleaning = self.clean_preview_task.is_some();

        let mut processing = is_analyzing || is_cleaning;
        if self.previews_needs_clean && !processing {
            processing = true;
            self.previews_needs_clean = false;

            let cleaner = ImageCleaner {
                speck_fill_color: self.preview_speck_fill_color,
                background_fill_color: self.preview_background_fill_color,
                ..self.cleaner
            };

            let analyzed_handle = self.analyzed_preview_image.clone();
            let cleaned_handle = self.cleaned_preview_image.clone();
            self.clean_preview_task = Some(tokio::spawn(async move {
                let analyzed = &*analyzed_handle.lock().unwrap();
                *cleaned_handle.lock().unwrap() = cleaner.clean(analyzed);
            }));
        }

        SidePanel::right("parameters").resizable(false).show(ctx, |ui| {
            ui.heading("Import parameters");
            Grid::new("import_parameters")
                .spacing([40.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label("Off-white threshold")
                        .on_hover_text("Pixels whose mean rgb value is lighter than this are considered off-white, and will be filled");
                    ui.add(Slider::new(&mut self.analyzer.off_white_threshold, 0..=255));
                    ui.end_row();

                    ui.label("Lightness thresholds")
                        .on_hover_text("Pixels whose mean rgb value is lighter than lightness and that don't have another pixel within distance (perpendicular distance) that is lighter than lightness will be filled");
                    ui.end_row();

                    ui.label("\t- Lightness");
                    ui.add(Slider::new(&mut self.analyzer.lightness_threshold, 0..=255));
                    ui.end_row();

                    ui.label("\t- Distance");
                    ui.add(Slider::new(&mut self.analyzer.lightness_distance, 0..=10));
                    ui.end_row();

                    if ui.button("Open images…").clicked() {
                        let extensions: Vec<&str> = [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::Tiff, ImageFormat::WebP].into_iter().flat_map(|f| f.extensions_str().iter().copied()).collect();
                        if let Some(paths) = rfd::FileDialog::new().add_filter("Image files", extensions.as_slice()).pick_files() {
                            self.on_images_update(paths);
                        }
                    }

                    if ui.button("Reimport").clicked() {
                        self.queue_analyze_preview();
                    }
                });

            ui.separator();

            ui.heading("Cleanup parameters");
            Grid::new("cleaner_parameters")
                .spacing([40.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label("Speck size threshold")
                        .on_hover_text("Clusters that have an area smaller than this will be filled");
                    if ui.add(Slider::new(&mut self.cleaner.speck_size_threshold, 0..=60).clamp_to_range(false).suffix("px²")).changed() {
                        self.queue_clean_preview();
                    }
                    ui.end_row();

                    ui.label("Speck margins")
                        .on_hover_text("Clusters that are within these margins will be filled");
                    ui.end_row();

                    ui.label("\t- x");
                    if ui.add(Slider::new(&mut self.cleaner.page_margins.0, 0..=100).clamp_to_range(false).suffix("px")).changed() {
                        self.queue_clean_preview();
                    }
                    ui.end_row();

                    ui.label("\t- y");
                    if ui.add(Slider::new(&mut self.cleaner.page_margins.1, 0..=100).clamp_to_range(false).suffix("px")).changed() {
                        self.queue_clean_preview();
                    }
                    ui.end_row();


                    ui.label("Isolation thresholds")
                        .on_hover_text("(Clusters that have an area smaller than this and aren't within this distance of another cluster that is will be filled");
                    ui.end_row();

                    ui.label("\t- Size");
                    if ui.add(Slider::new(&mut self.cleaner.isolation_size_threshold, 0..=150).clamp_to_range(false).suffix("px²")).changed() {
                        self.queue_clean_preview();
                    }
                    ui.end_row();
                    ui.label("\t- Distance");
                    if ui.add(Slider::new(&mut self.cleaner.isolation_distance_threshold, 0..=200).clamp_to_range(false).suffix("px")).changed() {
                        self.queue_clean_preview();
                    }
                    ui.end_row();

                    ui.label("Speck fill color")
                        .on_hover_text("What color to fill in specks (useful for debugging).");
                    if ui.color_edit_button_srgb(&mut self.cleaner.speck_fill_color).changed() {
                        self.queue_clean_preview();
                    }
                    ui.end_row();

                    ui.label("Background fill color")
                        .on_hover_text("What color to fill in the background (useful for debugging).");
                    if ui.color_edit_button_srgb(&mut self.cleaner.background_fill_color).changed() {
                        self.queue_clean_preview();
                    }
                    ui.end_row();

					if ui.add_enabled(!self.image_paths.is_empty() && self.export_task.is_none(), Button::new("Export all")).on_disabled_hover_text("No images have been opened or they are currently exporting").clicked() {
                        self.export_task = Some(tokio::spawn(Self::export_all(self.image_paths.clone(), self.analyzer, self.cleaner, self.export_progess.clone())));
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
                ui.heading("Preview parameters");
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
                            self.queue_clean_preview();
                        }
                        ui.end_row();

                        ui.label("Preview off-white fill color");
                        if ui
                            .color_edit_button_srgb(&mut self.preview_background_fill_color)
                            .changed()
                        {
                            self.queue_clean_preview();
                        }
                        ui.end_row();

                        ui.label("Preview off-white fill color");
                        ui.color_edit_button_srgba(&mut self.preview_margin_color);
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

                        ui.label("Offset");
                        ui.end_row();

                        ui.label("\t- x");
                        ui.add(DragValue::new(&mut self.preview_offset.x).suffix("px"));

                        ui.end_row();

                        ui.label("\t- y");
                        ui.add(DragValue::new(&mut self.preview_offset.y).suffix("px"));
                        ui.end_row();
                    });
            });

        CentralPanel::default()
            .frame(eframe::egui::Frame::none().fill(Color32::from_rgb(27, 26, 31)))
            .show(ctx, |ui| {
                if ui.max_rect().area() < 1.0 {
                    // Prevents a crash when shrinking the Panel
                    return;
                }

                // Use arrow keys to change page
                if ctx.input(|i| i.key_pressed(Key::ArrowRight))
                    && (self.preview_page as usize) < self.image_paths.len()
                {
                    self.preview_page += 1;
                    self.new_preview_image();
                } else if ctx.input(|i| i.key_pressed(Key::ArrowLeft)) && self.preview_page > 1 {
                    self.preview_page -= 1;
                    self.new_preview_image();
                }

                ui.set_clip_rect(ui.max_rect());

                let image_dimensions = Vec2::new(
                    self.preview_image_width as f32,
                    self.preview_image_height as f32,
                );

                // The ratio of whichever dimension has the largest difference between it and the available ui space (usually vertical for portrait pages)
                let largest_dimension = (image_dimensions.x / ui.available_width())
                    .max(image_dimensions.y / ui.available_height());
                let mut zoom = 2f32.powf(self.preview_zoom);
                let mut rect = Rect::ZERO;

                macro_rules! image_to_ui_scale {
                    ($v:expr) => {
                        $v / (largest_dimension / zoom)
                    };
                }

                macro_rules! ui_to_image_scale {
                    ($v:expr) => {
                        $v * (largest_dimension / zoom)
                    };
                }

                macro_rules! image_to_ui_pixels {
                    ($v:expr) => {
                        rect.left_top() + image_to_ui_scale!($v)
                    };
                }

                macro_rules! ui_to_image_pixels {
                    ($v:expr) => {
                        ui_to_image_scale!($v - rect.left_top())
                    };
                }

                // The rect that the preview image should be drawn in
                macro_rules! calc_ui_rect {
                    () => {
                        Rect::from_center_size(
                            ui.max_rect().center() + image_to_ui_scale!(self.preview_offset),
                            image_to_ui_scale!(image_dimensions),
                        )
                    };
                }

                rect = calc_ui_rect!();
                let mouse_pos =
                    ctx.input(|i| i.pointer.latest_pos().unwrap_or(ui.max_rect().center()));
                let mouse_hover_pixel = ui_to_image_pixels!(mouse_pos);

                let mut zooming = false;

                // Scroll to zoom
                let scroll_delta = ctx.input(|i| i.smooth_scroll_delta.y);
                let scrolling = scroll_delta.abs() > 0.05;
                if scrolling {
                    self.preview_zoom = self.preview_zoom + scroll_delta * self.preview_zoom_speed;
                    zooming = true;
                }
                if ui.input(|i| i.key_pressed(Key::Equals)) {
                    self.preview_zoom += 1.0;
                    zooming = true;
                }
                if ui.input(|i| i.key_pressed(Key::Minus)) {
                    self.preview_zoom -= 1.0;
                    zooming = true;
                }

                if zooming {
                    self.preview_zoom = self
                        .preview_zoom
                        .max(self.preview_min_zoom)
                        .min(self.preview_max_zoom);

                    // Stop velocity when zooming.
                    self.preview_velocity = Vec2::ZERO;
                    zoom = 2f32.powf(self.preview_zoom);

                    rect = calc_ui_rect!();
                    let new_mouse_hover_pixel = ui_to_image_pixels!(mouse_pos);
                    self.preview_offset += new_mouse_hover_pixel - mouse_hover_pixel;
                }

                // Drag to pan
                let content_response = ui.interact(ui.max_rect(), ui.id(), Sense::drag());
                if content_response.dragged() {
                    ui.input(|input| {
                        self.preview_offset += ui_to_image_scale!(input.pointer.delta());
                        self.preview_velocity = ui_to_image_scale!(input.pointer.velocity());
                    });
                } else {
                    // Kinetic panning
                    let stop_speed = 20.0; // Image pixels per second.
                    let friction_coeff = 1000.0; // Image pixels per second squared.
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

                // Clamp the preview offset
                self.preview_offset = self.preview_offset.clamp(
                    -(image_dimensions / 2.0) - ui_to_image_scale!(ui.max_rect().size()) / 4.0,
                    (image_dimensions / 2.0) + (ui_to_image_scale!(ui.max_rect().size()) / 4.0),
                );

                rect = calc_ui_rect!();

                let painter = ui.painter();

                painter.image(
                    self.preview_image_handle.id(),
                    rect,
                    Rect::from_x_y_ranges(0.0..=1.0, 0.0..=1.0),
                    Color32::WHITE,
                );

                // Draw margins
                for (a, b) in [
                    (
                        Vec2::ZERO,
                        Vec2::new(image_dimensions.x, self.cleaner.page_margins.1 as f32),
                    ),
                    (
                        Vec2::new(0.0, image_dimensions.y - self.cleaner.page_margins.1 as f32),
                        Vec2::new(image_dimensions.x, image_dimensions.y),
                    ),
                    (
                        Vec2::ZERO,
                        Vec2::new(self.cleaner.page_margins.0 as f32, image_dimensions.y),
                    ),
                    (
                        Vec2::new(image_dimensions.x - self.cleaner.page_margins.0 as f32, 0.0),
                        Vec2::new(image_dimensions.x, image_dimensions.y),
                    ),
                ] {
                    painter.rect_filled(
                        Rect::from_two_pos(image_to_ui_pixels!(a), image_to_ui_pixels!(b)),
                        0.0,
                        self.preview_margin_color,
                    );
                }

                if processing {
                    let spinner_radius = 50.0;
                    let spinner_inner_margin = 10.0;
                    let spinner_outer_margin = 10.0;

                    let spinner = Spinner::new();
                    painter.circle_filled(
                        ui.max_rect().left_top()
                            + Vec2::splat(spinner_radius + spinner_outer_margin),
                        spinner_radius,
                        Color32::from_black_alpha(128),
                    );
                    spinner.paint_at(
                        ui,
                        Rect::from_two_pos(
                            ui.max_rect().left_top()
                                + Vec2::splat(spinner_inner_margin + spinner_outer_margin),
                            ui.max_rect().left_top()
                                + Vec2::splat(
                                    (spinner_radius * 2.0 - spinner_inner_margin)
                                        + spinner_outer_margin,
                                ),
                        ),
                    );
                }
            });
    }
}
