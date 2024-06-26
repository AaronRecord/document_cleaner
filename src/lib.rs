use image::*;

#[derive(Clone, Copy)]
pub struct ImageCleaner {
    pub off_white_threshold: u8,
    pub lightness_threshold: u8,
    pub lightness_distance: u32,
    pub speck_size_threshold: usize,
    pub page_margins: (u32, u32),
    pub isolation_distance_threshold: u32,
    pub isolation_size_threshold: u32,
    pub speck_fill_color: [u8; 3],
    pub off_white_fill_color: [u8; 3],
}

impl Default for ImageCleaner {
    fn default() -> Self {
        Self {
            off_white_threshold: 240,
            speck_size_threshold: 15,
            lightness_threshold: 100,
            lightness_distance: 1,
            page_margins: (50, 50),
            isolation_distance_threshold: 50,
            isolation_size_threshold: 80,
            speck_fill_color: [255, 255, 255],
            off_white_fill_color: [255, 255, 255],
        }
    }
}

impl ImageCleaner {
    pub fn clean(&self, image: &DynamicImage) -> DynamicImage {
        let mut new_image: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::new(image.width(), image.height());

        // Whiten
        for (x, y, pixel) in image.pixels() {
            let value = pixel_value(pixel);

            // If the pixel isn't very dark and it's not next to other really dark pixels (like letter borders), fill it.
            if (value >= self.off_white_threshold)
                || (value >= self.lightness_threshold
                    && darkest_pixel_within(x, y, self.lightness_distance, image)
                        >= self.lightness_threshold)
            {
                new_image.put_pixel(
                    x,
                    y,
                    Rgba([
                        self.off_white_fill_color[0],
                        self.off_white_fill_color[1],
                        self.off_white_fill_color[2],
                        255,
                    ]),
                );
            }
        }

        let mut remaining_graphemes = Vec::new();
        for (x, y, _) in image.pixels() {
            if new_image.get_pixel(x, y)[3] != 0 {
                // White pixel.
                continue;
            }

            let grapheme = Grapheme::detect(x, y, image, &mut new_image);
            let too_small = grapheme.pixels.len() <= self.speck_size_threshold;
            let inside_margins = grapheme.top < self.page_margins.1
                || grapheme.bottom >= image.height() - self.page_margins.1
                || grapheme.left < self.page_margins.0
                || grapheme.right >= image.width() - self.page_margins.0;

            if too_small || inside_margins {
                // A speck/smudge probably.
                for pixel in grapheme.pixels {
                    new_image.put_pixel(
                        pixel.0,
                        pixel.1,
                        Rgba([
                            self.speck_fill_color[0],
                            self.speck_fill_color[1],
                            self.speck_fill_color[2],
                            255,
                        ]),
                    );
                }

                continue;
            }

            remaining_graphemes.push(grapheme);
        }

        for (grapheme_index, grapheme) in remaining_graphemes.iter().enumerate() {
            if grapheme.pixels.len() > self.isolation_size_threshold as usize {
                continue;
            }

            let mut isolated = true;

            for i in 0..remaining_graphemes.len() - 1 {
                // Iterate back and forth as an optimization, this way it searches by proximity.
                let negative = i % 2 == 1;
                let index =
                    grapheme_index as i64 + ((1 + i / 2) as i64 * if negative { -1 } else { 1 });
                let index = if index < 0 {
                    remaining_graphemes.len() - index.unsigned_abs() as usize
                } else if index >= remaining_graphemes.len() as i64 {
                    index as usize - remaining_graphemes.len()
                } else {
                    index as usize
                };

                let other_grapheme = &remaining_graphemes[index];
                let not_big_enough =
                    other_grapheme.pixels.len() < self.isolation_size_threshold as usize;

                // A speck needs to be close to a big grapheme to survive this, 2 small specks together won't survive.
                if not_big_enough {
                    continue;
                }

                let within_distance_threshold =
                    (positive_difference(grapheme.top, other_grapheme.top)
                        < self.isolation_distance_threshold
                        || positive_difference(grapheme.bottom, other_grapheme.bottom)
                            < self.isolation_distance_threshold)
                        && (positive_difference(grapheme.left, other_grapheme.left)
                            < self.isolation_distance_threshold
                            || positive_difference(grapheme.right, other_grapheme.right)
                                < self.isolation_distance_threshold);

                if within_distance_threshold {
                    isolated = false;
                    break;
                }
            }

            if !isolated {
                continue;
            }

            // A speck/smudge probably.
            for pixel in grapheme.pixels.iter() {
                new_image.put_pixel(
                    pixel.0,
                    pixel.1,
                    Rgba([
                        self.speck_fill_color[0],
                        self.speck_fill_color[1],
                        self.speck_fill_color[2],
                        255,
                    ]),
                );
            }
        }

        DynamicImage::ImageRgba8(new_image)
    }
}

struct Grapheme {
    pixels: Vec<(u32, u32)>,
    top: u32,
    bottom: u32,
    left: u32,
    right: u32,
    average_value: u8,
}

impl Grapheme {
    fn detect(
        x: u32,
        y: u32,
        image: &DynamicImage,
        new_image: &mut ImageBuffer<Rgba<u8>, Vec<u8>>,
    ) -> Self {
        const NEIGHBORS: [(i32, i32); 4] = [
            (1, 0),
            //(1, 1),
            (0, 1),
            //(-1, 1),
            (-1, 0),
            //(-1, -1),
            (0, -1),
            //(1, -1),
        ];

        let mut grapheme = Self {
            pixels: Vec::new(),
            top: y,
            bottom: y,
            left: x,
            right: x,
            average_value: 0,
        };

        let mut stack = Vec::new();
        stack.push((x, y));
        new_image.put_pixel(x, y, image.get_pixel(x, y));

        while let Some((x, y)) = stack.pop() {
            grapheme.pixels.push((x, y));

            if x < grapheme.left {
                grapheme.left = x;
            }
            if x > grapheme.right {
                grapheme.right = x;
            }
            if y < grapheme.top {
                grapheme.top = y;
            }
            if y > grapheme.bottom {
                grapheme.bottom = y;
            }

            for neighbor in NEIGHBORS {
                let (x, y) = (
                    (x as i32 + neighbor.0) as u32,
                    (y as i32 + neighbor.1) as u32,
                );
                if x >= image.width() || y >= image.height() {
                    continue;
                }

                if new_image.get_pixel(x, y)[3] != 0 {
                    // Use alpha to determine if the pixel is in the open or closed set.
                    continue;
                }

                new_image.put_pixel(x, y, image.get_pixel(x, y));
                stack.push((x, y));
            }
        }

        let mut total: u32 = 0;
        for (x, y) in grapheme.pixels.iter() {
            let pixel = image.get_pixel(*x, *y);
            total += pixel_value(pixel) as u32;
        }

        grapheme.average_value = (total / grapheme.pixels.len() as u32) as u8;

        grapheme
    }
}

fn positive_difference(a: u32, b: u32) -> u32 {
    if a >= b {
        a - b
    } else {
        b - a
    }
}

fn darkest_pixel_within(x: u32, y: u32, distance: u32, image: &DynamicImage) -> u8 {
    //for pixel in image.view(x - distance, y - distance, distance * 2, distance * 2);
    let mut darkest: u8 = 255;
    for y in (y - distance).max(0)..=(y + distance).min(image.height() - 1) {
        for x in (x - distance).max(0)..=(x + distance).min(image.width() - 1) {
            let pixel = pixel_value(image.get_pixel(x, y));
            if pixel < darkest {
                darkest = pixel;
            }
        }
    }

    darkest
}

fn pixel_value(pixel: Rgba<u8>) -> u8 {
    ((pixel[0] as u32 + pixel[1] as u32 + pixel[2] as u32) / 3) as u8
}
