#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;
use eframe::egui::*;
use image::*;
use image_cleanup::ImageCleaner;
use tokio;

#[tokio::main]
async fn main() -> Result<(), eframe::Error> {
    env_logger::init(); // Log to stderr (if you run with `RUST_LOG=debug`).
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default().with_maximized(true),
        ..Default::default()
    };

    eframe::run_native(
        "Image Cleanup",
        options,
        Box::new(|cc| Box::new(ImageCleanup::new(&cc.egui_ctx))),
    )
}

static DEMO_IMAGE_DATA: &'static [u8] = include_bytes!("../assets/demo_image.png");

struct ImageCleanup {
    image_cleaner: ImageCleaner,
    original_preview_image: DynamicImage,
    preview_image_handle: TextureHandle,
    processing_preview_image: Arc<Mutex<Option<DynamicImage>>>,
    image_paths: Vec<PathBuf>,
    preview_page: u16,
}

fn dynamic_image_to_handle(
    ctx: &Context,
    name: impl Into<String>,
    image: &DynamicImage,
) -> TextureHandle {
    let size = [image.width() as _, image.height() as _];
    let image_buffer = image.to_rgba8();
    let pixels = image_buffer.as_flat_samples();
    let image = ColorImage::from_rgba_unmultiplied(size, pixels.as_slice());
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
    image::load_from_memory_with_format(DEMO_IMAGE_DATA, ImageFormat::Png).unwrap()
}

impl ImageCleanup {
    fn new(ctx: &Context) -> Self {
        let image_cleaner = ImageCleaner::default();
        let original_preview_image = demo_image();
        let preview_image_handle =
            dynamic_image_to_handle(ctx, "preview_image", &original_preview_image);

        let processing_preview_image = Arc::new(Mutex::new(None));
        let result_for_thread = processing_preview_image.clone();
        tokio::spawn(async move {
            let preview_image = image_cleaner.clean(&original_preview_image);
            let mut mutex_lock = result_for_thread.lock().unwrap();
            *mutex_lock = Some(preview_image);
        });

        Self {
            image_cleaner,
            preview_page: 1,
            processing_preview_image,
            preview_image_handle,
            image_paths: Vec::new(),
            original_preview_image,
        }
    }

    fn on_images_update(&mut self, ctx: &Context, paths: Vec<PathBuf>) {
        self.image_paths = paths;
        self.new_preview_image(ctx);
    }

    fn new_preview_image(&mut self, ctx: &Context) {
        self.original_preview_image = if !self.image_paths.is_empty() {
            image::io::Reader::open(&self.image_paths[(self.preview_page - 1) as usize])
                .unwrap()
                .decode()
                .unwrap()
        } else {
            demo_image()
        };

        self.clean_preview(ctx);
    }

    fn clean_preview(&mut self, ctx: &Context) {
        tokio::spawn(async move {
            let preview = self.image_cleaner.clean(&self.original_preview_image);
            self.processing_preview_image = Arc::new(Mutex::new(Some()));
        })
        .await;
    }
}

impl eframe::App for ImageCleanup {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        SidePanel::left("preview")
            .default_width(1440.0)
            .show(ctx, |ui| {
                SidePanel::left("preview_tools").show_inside(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Preview Page");
                        if ui
                            .add(
                                DragValue::new(&mut self.preview_page)
                                    .clamp_range(1..=self.image_paths.len().at_least(1)),
                            )
                            .changed()
                        {
                            self.new_preview_image(ctx);
                        }
                    });
                });

                ScrollArea::both().show(ui, |ui| {
                    if let Ok(mutex) = self.processing_preview_image.try_lock() {
                        if let Some(handle) = (*mutex).clone() {
                            let preview_image = Image::new(&handle);
                            ui.add(preview_image);
                        } else {
                            ui.spinner();
                        }
                    }
                });
            });

        CentralPanel::default().show(ctx, |ui| {
            Grid::new("parameters")
                .num_columns(2)
                .spacing([40.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    if ui.button("Open images…").clicked() {
						let extensions: Vec<&str> = [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::Tiff, ImageFormat::WebP].into_iter().flat_map(|f| f.extensions_str().iter().copied()).collect();
                        if let Some(paths) = rfd::FileDialog::new().add_filter("Image files", extensions.as_slice()).pick_files() {
                            self.on_images_update(ctx, paths);
                        }
                    }
                    ui.end_row();

                    ui.label("Off-white threshold")
                        .on_hover_text("Colors with their r, g, and b values greater than this are considered off-white, and will be filled");
                    if ui.add(Slider::new(&mut self.image_cleaner.off_white_threshold, 0..=255)).changed() {
                        self.clean_preview(ctx);
                    }

                    ui.label("Speck size threshold")
                        .on_hover_text("Clusters that have an area smaller than this will be filled");
                    if ui.add(Slider::new(&mut self.image_cleaner.speck_size_threshold, 0..=100).clamp_to_range(false).suffix("px")).changed() {
                        self.clean_preview(ctx);
                    }

                    ui.label("Speck lightness threshold")
                        .on_hover_text("Clusters that have an average rgb value greater than this will be filled");
                    if ui.add(Slider::new(&mut self.image_cleaner.speck_lightness_threshold, 0..=255)).changed() {
                        self.clean_preview(ctx);
                    }

                    Grid::new("speck_margins").show(ui, |ui| {
                        ui.label("Speck margins")
                            .on_hover_text("Clusters that are within these margins will be filled");
                        ui.end_row();
                        ui.label("x");
                        if ui.add(Slider::new(&mut self.image_cleaner.speck_margins.0, 0..=100).clamp_to_range(false).suffix("px")).changed() {
                            self.clean_preview(ctx);
                        }
                        ui.end_row();
                        ui.label("y");
                        if ui.add(Slider::new(&mut self.image_cleaner.speck_margins.1, 0..=100).clamp_to_range(false).suffix("px")).changed() {
                            self.clean_preview(ctx);
                        }
					});
                    ui.end_row();

                    Grid::new("isolation_thresholds").show(ui, |ui| {
                        ui.label("Isolation thresholds")
                            .on_hover_text(" (Clusters that have an area smaller than this and aren't within this distance of another cluster that is will be filled");
                        ui.end_row();
                        ui.label("size");
                                            if ui.add(Slider::new(&mut self.image_cleaner.isolation_size_threshold, 0..=150).clamp_to_range(false).suffix("px")).changed() {
                                                self.clean_preview(ctx);
                                            }
                                            ui.end_row();
                                            ui.label("distance");
                                            if ui.add(Slider::new(&mut self.image_cleaner.isolation_distance_threshold, 0..=200).clamp_to_range(false).suffix("px")).changed() {
                                                self.clean_preview(ctx);
                                            }

                    });
                    ui.end_row();

                    ui.label("Speck fill color")
                        .on_hover_text("What color to fill in specks, useful for verification");
                    if ui.color_edit_button_srgb(&mut self.image_cleaner.speck_fill_color).changed() {
                        self.clean_preview(ctx);
                    }

                    ui.label("Off-white fill color")
                        .on_hover_text("What color to fill in off-white pixels, useful for verification");
                                    if ui.color_edit_button_srgb(&mut self.image_cleaner.off_white_fill_color).changed() {
                                        self.clean_preview(ctx);
                                    }

					if ui.add_enabled(!self.image_paths.is_empty(), Button::new("Export all")).on_disabled_hover_text("No images have been opened").clicked() {
						for path in &self.image_paths {
							let image = image::io::Reader::open(path).unwrap().decode().unwrap();
							let cleaned_image = self.image_cleaner.clean(&image);
							cleaned_image.save(path).unwrap();
						}
					}
                });
        });
    }
}
