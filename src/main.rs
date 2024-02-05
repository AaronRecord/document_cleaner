use image::ImageFormat;
use image::*;
use std::fs::{self, File};
use std::io::{prelude::*, stdout};
use std::path::{Path, PathBuf};

fn main() -> Result<(), ()> {
    let mut args = std::env::args()
        .skip(1)
        .map(|s| {
            s.parse::<usize>()
                .expect("Expected argument to be a postive integer")
        })
        .collect::<Vec<usize>>()
        .into_iter();

    let off_white_threshold: u8 = args.next().unwrap_or(230) as u8;
    let speck_size_threshold: usize = args.next().unwrap_or(35);
    let speck_lightness_threshold: u8 = args.next().unwrap_or(100) as u8;
    let speck_margins = (
        args.next().unwrap_or(50) as u32,
        args.next().unwrap_or(50) as u32,
    );
    let isolation_distance_threshold = args.next().unwrap_or(50) as u32;
    let isolation_size_threshold = args.next().unwrap_or(64) as u32;
    let speck_fill_color = [
        args.next().unwrap_or(255) as u8,
        args.next().unwrap_or(255) as u8,
        args.next().unwrap_or(255) as u8,
    ];
    let off_white_fill_color = [
        args.next().unwrap_or(255) as u8,
        args.next().unwrap_or(255) as u8,
        args.next().unwrap_or(255) as u8,
    ];

    println!(
        "Off-white threshold (colors with their r, g, and b values greater than this are considered off-white, and will be filled): {},
Speck size threshold (clusters that have an area smaller than this will be filled): {}px,
Speck lightness threshold (clusters that have an average rgb value greater than this will be filled): {},
Speck margins (clusters that are within these margins will be filled): (horizontal: {}px, vertical: {}px),
Isolation thresholds (clusters that have an area smaller than this and aren't within this distance of another cluster that is will be filled): (distance: {}px, size: {}px),
Speck fill color (what color to fill in specks, useful for verification): (r: {}, g: {}, b: {}),
Off-white fill color (what color to fill in off-white pixels, useful for verification): (r: {}, g: {}, b: {})\n",
        off_white_threshold,
        speck_size_threshold,
        speck_lightness_threshold,
        speck_margins.0,
        speck_margins.1,
		isolation_distance_threshold,
		isolation_size_threshold,
        speck_fill_color[0],
        speck_fill_color[1],
        speck_fill_color[2],
        off_white_fill_color[0],
        off_white_fill_color[1],
        off_white_fill_color[2],
    );

    // Open the file picker dialog to select a PDF file
    let document_pages = tinyfiledialogs::open_file_dialog_multi(
        "Select document pages",
        "",
        Some((&["*.png"], "PNG files")),
    )
    .unwrap();

    for (i, page) in document_pages.iter().enumerate() {
        print!(
            "\r{}% ({} / {})",
            (((i) as f64 / document_pages.len() as f64) * 100.0) as u64,
            i,
            document_pages.len(),
        );
        stdout().flush().unwrap();

        let image = image::open(page).unwrap();
        let mut new_image: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::new(image.width(), image.height());

        for (x, y, pixel) in image.pixels() {
            if pixel[0] > off_white_threshold
                && pixel[1] > off_white_threshold
                && pixel[2] > off_white_threshold
            {
                new_image.put_pixel(
                    x,
                    y,
                    Rgba([
                        off_white_fill_color[0],
                        off_white_fill_color[1],
                        off_white_fill_color[2],
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

            let grapheme = detect_grapheme(x, y, &image, &mut new_image);
            let too_small = grapheme.pixels.len() <= speck_size_threshold;
            let inside_margins = grapheme.top < speck_margins.1
                || grapheme.bottom >= image.height() - speck_margins.1
                || grapheme.left < speck_margins.0
                || grapheme.right >= image.width() - speck_margins.0;
            let not_dark_enough = grapheme.average_value > speck_lightness_threshold;

            if too_small || inside_margins || not_dark_enough {
                //println!("Speck removed at x: {}, y: {} too_small: {}, inside_margins: {}, not_dark_enough: {}", grapheme.left, grapheme.top, too_small, inside_margins, not_dark_enough);

                // A speck/smudge probably.
                for pixel in grapheme.pixels {
                    new_image.put_pixel(
                        pixel.0,
                        pixel.1,
                        Rgba([
                            speck_fill_color[0],
                            speck_fill_color[1],
                            speck_fill_color[2],
                            255,
                        ]),
                    );
                }

                continue;
            }

            remaining_graphemes.push(grapheme);
        }

        for (grapheme_index, grapheme) in remaining_graphemes.iter().enumerate() {
            if grapheme.pixels.len() > isolation_size_threshold as usize {
                continue;
            }

            let mut isolated = true;

            for i in 0..remaining_graphemes.len() - 1 {
                // Iterate back and forth as an optimization, this way it searches by proximity.
                let negative = i % 2 == 1;
                let index =
                    grapheme_index as i64 + ((1 + i / 2) as i64 * if negative { -1 } else { 1 });
                let index = if index < 0 {
                    remaining_graphemes.len() - index.abs() as usize
                } else if index >= remaining_graphemes.len() as i64 {
                    index as usize - remaining_graphemes.len()
                } else {
                    index as usize
                };

                let other_grapheme = &remaining_graphemes[index];
                let not_big_enough =
                    other_grapheme.pixels.len() < isolation_size_threshold as usize;

                // A speck needs to be close to a big grapheme to survive this, 2 small specks together won't survive.
                if not_big_enough {
                    continue;
                }

                let within_distance_threshold =
                    (positive_difference(grapheme.top, other_grapheme.top)
                        < isolation_distance_threshold
                        || positive_difference(grapheme.bottom, other_grapheme.bottom)
                            < isolation_distance_threshold)
                        && (positive_difference(grapheme.left, other_grapheme.left)
                            < isolation_distance_threshold
                            || positive_difference(grapheme.right, other_grapheme.right)
                                < isolation_distance_threshold);

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
                        speck_fill_color[0],
                        speck_fill_color[1],
                        speck_fill_color[2],
                        255,
                    ]),
                );
            }
        }

        new_image.save(page).unwrap();
    }

    print!("\rProgram successfully finished, press enter to close...");
    stdout().flush().unwrap();
    std::io::stdin().read_line(&mut String::new()).unwrap();

    Ok(())
}

struct Grapheme {
    pixels: Vec<(u32, u32)>,
    top: u32,
    bottom: u32,
    left: u32,
    right: u32,
    average_value: u8,
}

fn detect_grapheme(
    x: u32,
    y: u32,
    image: &DynamicImage,
    new_image: &mut ImageBuffer<Rgba<u8>, Vec<u8>>,
) -> Grapheme {
    const NEIGHBORS: [(i32, i32); 8] = [
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
        (-1, 0),
        (-1, -1),
        (0, -1),
        (1, -1),
    ];

    let mut grapheme = Grapheme {
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

    let mut total: usize = 0;
    for (x, y) in grapheme.pixels.iter() {
        let pixel = image.get_pixel(*x, *y);
        total += (pixel[0] as usize + pixel[1] as usize + pixel[2] as usize) / 3;
    }

    grapheme.average_value = (total / grapheme.pixels.len()) as u8;

    grapheme
}

fn positive_difference(a: u32, b: u32) -> u32 {
    if a >= b {
        a - b
    } else {
        b - a
    }
}
