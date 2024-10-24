use image::*;

#[derive(Clone, Copy)]
pub struct ImageAnalyzer {
    pub off_white_threshold: u8,
    pub lightness_threshold: u8,
    pub lightness_distance: u32,
}

impl Default for ImageAnalyzer {
    fn default() -> Self {
        Self {
            off_white_threshold: 240,
            lightness_threshold: 100,
            lightness_distance: 1,
        }
    }
}

#[derive(Clone, Copy)]
pub struct ImageCleaner {
    pub speck_size_threshold: usize,
    pub page_margins: (u32, u32),
    pub isolation_distance_threshold: u32,
    pub isolation_size_threshold: u32,
    pub speck_fill_color: [u8; 3],
    pub background_fill_color: [u8; 3],
}

impl Default for ImageCleaner {
    fn default() -> Self {
        Self {
            speck_size_threshold: 15,
            page_margins: (50, 50),
            isolation_distance_threshold: 50,
            isolation_size_threshold: 80,
            speck_fill_color: [255, 255, 255],
            background_fill_color: [255, 255, 255],
        }
    }
}

pub struct AnalyzedImage {
    pub graphemes: Vec<Grapheme>,
    pub map: Vec<u32>,
    pub width: u32,
    pub height: u32,
}

impl AnalyzedImage {
    fn new(image: &RgbImage) -> Self {
        Self {
            map: vec![u32::MAX; (image.width() * image.height()) as usize],
            graphemes: Vec::new(),
            width: image.width(),
            height: image.height(),
        }
    }

    pub fn get_grapheme_at(&self, x: u32, y: u32) -> Option<&Grapheme> {
        let i = match self.map[(self.width * y + x) as usize] {
            u32::MAX => None,
            i => Some(i),
        }? as usize;
        Some(&self.graphemes[i])
    }

    fn set_grapheme_at(&mut self, x: u32, y: u32, i: Option<u32>) {
        self.map[(self.width * y + x) as usize] = i.unwrap_or(u32::MAX);
    }
}

struct VisitedMap {
    map: Vec<bool>,
    width: u32,
    _height: u32,
}

impl VisitedMap {
    fn new(width: u32, height: u32) -> Self {
        Self {
            map: vec![false; (width * height) as usize],
            width,
            _height: height,
        }
    }

    fn is_visited(&self, x: u32, y: u32) -> bool {
        self.map[(y * self.width + x) as usize]
    }

    fn set_visited(&mut self, x: u32, y: u32, b: bool) {
        self.map[(y * self.width + x) as usize] = b;
    }
}

impl ImageAnalyzer {
    pub fn analyze(&self, image: &RgbImage) -> AnalyzedImage {
        let mut analyzed_image = AnalyzedImage::new(image);
        let mut visited_map = VisitedMap::new(image.width(), image.height());

        // Whiten
        for (x, y, pixel) in image.enumerate_pixels() {
            let value = pixel_value(*pixel);

            // If the pixel isn't very dark and it's not next to other really dark pixels (like letter borders), fill it.
            let offwhite = value >= self.off_white_threshold;
            let too_light_and_distant = value >= self.lightness_threshold
                && darkest_pixel_within(x, y, self.lightness_distance, image)
                    >= self.lightness_threshold;

            if offwhite || too_light_and_distant {
                visited_map.set_visited(x, y, true);
            }
        }

        for (x, y, _) in image.enumerate_pixels() {
            if visited_map.is_visited(x, y) {
                continue;
            }

            let grapheme = Grapheme::detect(x, y, image, &mut visited_map);
            for (x, y, _) in grapheme.pixels.iter() {
                analyzed_image.set_grapheme_at(*x, *y, Some(analyzed_image.graphemes.len() as u32));
            }
            analyzed_image.graphemes.push(grapheme);
        }

        analyzed_image
    }
}

impl ImageCleaner {
    pub fn clean(&self, analyzed_image: &AnalyzedImage) -> RgbImage {
        let mut new_image: RgbImage = ImageBuffer::new(analyzed_image.width, analyzed_image.height);
        for p in new_image.pixels_mut() {
            *p = self.background_fill_color.into();
        }

        for (i, grapheme) in analyzed_image.graphemes.iter().enumerate() {
            if let Some(manual_override) = grapheme.manual_override {
                match manual_override {
                    false => grapheme.fill(&mut new_image, self.speck_fill_color.into()),
                    true => grapheme.draw(&mut new_image),
                }

                continue;
            }

            let too_small = grapheme.pixels.len() <= self.speck_size_threshold;
            let inside_margins = grapheme.top < self.page_margins.1
                || grapheme.bottom >= analyzed_image.height - self.page_margins.1
                || grapheme.left < self.page_margins.0
                || grapheme.right >= analyzed_image.width - self.page_margins.0;
            let is_isolated = self.is_isolated(i, &analyzed_image.graphemes);

            if too_small || inside_margins || is_isolated {
                // A speck/smudge probably.
                grapheme.fill(&mut new_image, self.speck_fill_color.into())
            } else {
                grapheme.draw(&mut new_image);
            }
        }

        new_image
    }

    fn is_isolated(&self, grapheme_index: usize, graphemes: &[Grapheme]) -> bool {
        let grapheme = &graphemes[grapheme_index];
        if grapheme.pixels.len() > self.isolation_size_threshold as usize {
            return false;
        }

        for i in 0..graphemes.len() - 1 {
            // Iterate back and forth as an optimization, this way it searches by proximity.
            let negative = i % 2 == 1;
            let index =
                grapheme_index as i64 + ((1 + i / 2) as i64 * if negative { -1 } else { 1 });
            let index = if index < 0 {
                graphemes.len() - index.unsigned_abs() as usize
            } else if index >= graphemes.len() as i64 {
                index as usize - graphemes.len()
            } else {
                index as usize
            };

            let other_grapheme = &graphemes[index];
            let not_big_enough =
                other_grapheme.pixels.len() < self.isolation_size_threshold as usize;

            // A speck needs to be close to a big grapheme to survive this, 2 small specks together won't survive.
            if not_big_enough {
                continue;
            }

            let within_distance_threshold = (positive_difference(grapheme.top, other_grapheme.top)
                < self.isolation_distance_threshold
                || positive_difference(grapheme.bottom, other_grapheme.bottom)
                    < self.isolation_distance_threshold)
                && (positive_difference(grapheme.left, other_grapheme.left)
                    < self.isolation_distance_threshold
                    || positive_difference(grapheme.right, other_grapheme.right)
                        < self.isolation_distance_threshold);

            if within_distance_threshold {
                return false;
            }
        }

        true
    }
}

pub struct Grapheme {
    pixels: Vec<(u32, u32, Rgb<u8>)>,
    top: u32,
    bottom: u32,
    left: u32,
    right: u32,
    // If true, always draw no matter what, if false, never draw no matter what.
    manual_override: Option<bool>,
}

impl Grapheme {
    fn detect(x: u32, y: u32, image: &RgbImage, visited_map: &mut VisitedMap) -> Self {
        const NEIGHBORS: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];

        let mut grapheme = Self {
            pixels: Vec::new(),
            top: y,
            bottom: y,
            left: x,
            right: x,
            manual_override: None,
        };

        let mut stack = Vec::new();
        visited_map.set_visited(x, y, true);
        stack.push((x, y));

        while let Some((x, y)) = stack.pop() {
            grapheme.pixels.push((x, y, *image.get_pixel(x, y)));

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

                if visited_map.is_visited(x, y) {
                    continue;
                }

                visited_map.set_visited(x, y, true);
                stack.push((x, y));
            }
        }

        grapheme
    }

    fn _average_value(&self) -> u8 {
        let mut total: u32 = 0;
        for (_, _, v) in self.pixels.iter() {
            total += pixel_value(*v) as u32;
        }

        (total / self.pixels.len() as u32) as u8
    }

    fn fill(&self, image: &mut RgbImage, color: Rgb<u8>) {
        for (x, y, _) in &self.pixels {
            image.put_pixel(*x, *y, color);
        }
    }

    fn draw(&self, image: &mut RgbImage) {
        for (x, y, c) in &self.pixels {
            image.put_pixel(*x, *y, *c);
        }
    }
}

fn positive_difference(a: u32, b: u32) -> u32 {
    if a >= b {
        a - b
    } else {
        b - a
    }
}

fn darkest_pixel_within(x: u32, y: u32, distance: u32, image: &RgbImage) -> u8 {
    //for pixel in image.view(x - distance, y - distance, distance * 2, distance * 2);
    let mut darkest: u8 = 255;
    for y in (y - distance).max(0)..=(y + distance).min(image.height() - 1) {
        for x in (x - distance).max(0)..=(x + distance).min(image.width() - 1) {
            let pixel = pixel_value(*image.get_pixel(x, y));
            if pixel < darkest {
                darkest = pixel;
            }
        }
    }

    darkest
}

fn pixel_value(pixel: Rgb<u8>) -> u8 {
    ((pixel[0] as u32 + pixel[1] as u32 + pixel[2] as u32) / 3) as u8
}
