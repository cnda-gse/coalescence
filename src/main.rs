/*
coalescence
Copyright (C) 2026 Devon Shelton

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/
pub mod weather;
pub mod astro;

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use escpos::printer::Printer;
use escpos::printer_options::PrinterOptions;
use escpos::utils::*;
use escpos::{driver::*, errors::Result};
use image::ExtendedColorType::L8;
use image::ImageFormat::Png;
use image::imageops::{overlay, dither, BiLevel};
use image::{DynamicImage, GrayImage, Luma, save_buffer_with_format};
use imageproc::drawing::{
    draw_filled_rect_mut, draw_hollow_rect_mut, draw_line_segment_mut, draw_text_mut,
};
use imageproc::rect::Rect;
use jiff::{Unit, Zoned};
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::Cursor;
use std::sync::OnceLock;
use std::thread::sleep;
use std::time::Duration;
use spinners::{Spinner, Spinners};

// Embedded fonts available everywhere across modules
pub static UNIFONT_BYTES: &[u8] = include_bytes!("../fonts/UnifontExMono.ttf");
pub static UNIFONT_UPPER_BYTES: &[u8] = include_bytes!("../fonts/unifont_upper-18.0.01.otf");

static CONFIG: OnceLock<Config> = OnceLock::new();
static COORDS: OnceLock<(f64, f64, Option<String>)> = OnceLock::new();

type ImageMacroFn = fn(&[&str]) -> GrayImage;
type TextMacroFn = fn(&[&str]) -> String;

#[derive(Debug, PartialEq)]
pub enum Token {
    Text(String),
    Bold(bool),
    Italic(bool),
    Underline(bool),
    Reverse(bool),
    Height(usize),
    Width(usize),
    VSpace(u32),
    HLine(u32),
    DynamicImage(image::GrayImage),
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub profile: Profile,
    pub template: Template,
}

#[derive(Debug, Deserialize)]
pub struct Profile {
    pub location: String,
}

#[derive(Debug, Deserialize)]
pub struct Template {
    pub width: u32,
    pub content: String,
}

fn main() -> std::io::Result<()> {
    println!("
    coalescence Copyright (C) 2026 Devon Shelton
    This program comes with ABSOLUTELY NO WARRANTY. This is free software, and you are welcome to redistribute it under certain conditions.
    ");
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: {} <template_file>", args[0]);
        std::process::exit(1);
    }

    let file_path = &args[1];
    let config = read_toml(file_path);
    CONFIG.set(config).expect("ERROR ] Failed to set CONFIG!");

    // Resolve geographic coordinates for location in profile
    let loc_str = &CONFIG.get().unwrap().profile.location;

    println!("\
    LOG ] Constructing with following configuration:
    WIDTH: {}px
    DEFAULT LOCATION: {}", &(CONFIG.get().unwrap()).template.width, loc_str);

    let mut sp = Spinner::new(Spinners::Dots9, "Resolving location...".into());
    sleep(Duration::from_secs(3));

    let (lat, lon, tz) = match weather::fetch_location(loc_str) {
        Ok(geo) => {
            sp.stop_and_persist(">", format!("Resolved \"{}\" as ({:.2}), ({:.2})", loc_str, geo.latitude, geo.longitude));
            sleep(Duration::from_secs(2));
            (geo.latitude, geo.longitude, geo.timezone)
        }
        Err(e) => {
            eprintln!("ERROR ] Could not resolve \"{}\"': {}!", loc_str, e);
            (52.52, 13.405, None)
        }
    };
    COORDS.set((lat, lon, tz)).expect("LOG ] Failed to set COORDS");

    // Initialize Macro Registry with parameter support
    let mut image_macros: HashMap<&str, ImageMacroFn> = HashMap::new();
    image_macros.insert("!weather", get_weather);
    image_macros.insert("!astro", get_astro);
    image_macros.insert("!image", get_image);

    let mut text_macros: HashMap<&str, TextMacroFn> = HashMap::new();
    text_macros.insert("#date", get_date);
    text_macros.insert("#location", get_location);

    let template = &(CONFIG.get().unwrap()).template.content;
    let tokens = tokenize(template, &image_macros, &text_macros);

    // Draw dynamic image canvas sized to content
    let _img = draw_image(tokens, &(CONFIG.get().unwrap()).template.width);

    //print_image(_img).expect("TODO: printer error handling");

    Ok(())
}

// --- Custom Text Measurement ---
pub fn text_size(scale: PxScale, font: &impl Font, text: &str) -> (u32, u32) {
    let scaled_font = font.as_scaled(scale);
    let mut width = 0.0;
    let mut last_glyph = None;

    for c in text.chars() {
        let glyph_id = font.glyph_id(c);
        if let Some(last) = last_glyph {
            width += scaled_font.kern(last, glyph_id);
        }
        width += scaled_font.h_advance(glyph_id);
        last_glyph = Some(glyph_id);
    }

    (width.ceil() as u32, scaled_font.height().ceil() as u32)
}

// --- Image Macros with Parameter Support ---

// Generates weather report. Optional arg[0]: custom location string (e.g. "Tokyo" or "London").
fn get_weather(args: &[&str]) -> GrayImage {
    let loc_str = if !args.is_empty() {
        args[0]
    } else {
        &CONFIG.get().unwrap().profile.location
    };

    match weather::fetch_location(loc_str) {
        Ok(geo) => {
            println!("LOG ] WEATHER ran for '{}': ({:.2}, {:.2})", loc_str, geo.latitude, geo.longitude);
            weather::generate_with_coords(geo.latitude, geo.longitude, loc_str)
        }
        Err(e) => {
            eprintln!("ERROR ] WEATHER failed to geocode '{}': {}!", loc_str, e);
            let (lat, lon, _) = COORDS.get().unwrap();
            weather::generate_with_coords(*lat, *lon, loc_str)
        }
    }
}

// Generates astronomical report. Optional arg[0]: custom location string.
fn get_astro(args: &[&str]) -> GrayImage {
    let loc_str = if !args.is_empty() {
        args[0]
    } else {
        &CONFIG.get().unwrap().profile.location
    };

    match weather::fetch_location(loc_str) {
        Ok(geo) => {
            println!("LOG ] ASTRO ran for '{}': ({:.2}, {:.2}), tz: {:?}", loc_str, geo.latitude, geo.longitude, geo.timezone);
            astro::generate(geo.latitude, geo.longitude, geo.timezone.as_deref())
        }
        Err(e) => {
            eprintln!("ERROR ] ASTRO failed to geocode '{}': {}!", loc_str, e);
            let (lat, lon, tz) = COORDS.get().unwrap();
            astro::generate(*lat, *lon, tz.as_deref())
        }
    }
}

// Margin in CSS style (Top Right Bottom Left).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Margin {
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
    pub left: u32,
}

impl Margin {
    // Parse CSS shorthand margins from tag argument slices:
    // - 1 value: [all] -> T=R=B=L
    // - 2 values: [tb, rl] -> T=B=tb, R=L=rl
    // - 3 values: [top, rl, bottom] -> T=top, R=L=rl, B=bottom
    // - 4 values: [top, right, bottom, left] -> T=top, R=right, B=bottom, L=left
    pub fn parse(args: &[&str]) -> Self {
        let mut numbers: Vec<u32> = Vec::new();
        for arg in args {
            for part in arg.split_whitespace() {
                let cleaned = part.trim_end_matches("px");
                if let Ok(val) = cleaned.parse::<u32>() {
                    numbers.push(val);
                }
            }
        }

        match numbers.as_slice() {
            [] => Margin::default(),
            [all] => Margin {
                top: *all,
                right: *all,
                bottom: *all,
                left: *all,
            },
            [tb, rl] => Margin {
                top: *tb,
                right: *rl,
                bottom: *tb,
                left: *rl,
            },
            [top, rl, bottom] => Margin {
                top: *top,
                right: *rl,
                bottom: *bottom,
                left: *rl,
            },
            [top, right, bottom, left, ..] => Margin {
                top: *top,
                right: *right,
                bottom: *bottom,
                left: *left,
            },
        }
    }
}

// Injects an image file. arg[0]: path.
// Remaining arguments are parsed as CSS-style margins (T R B L):
// e.g. {{ !image "pic.png" 10 20 10 20 }} or {{ !image "pic.png" "10 20" }}
fn get_image(args: &[&str]) -> GrayImage {
    if args.is_empty() {
        eprintln!("LOG ] !image macro requires at least one argument (file path)");
        let mut err_img = GrayImage::from_pixel((CONFIG.get().unwrap()).template.width, 40, Luma([255]));
        let font = FontRef::try_from_slice(UNIFONT_BYTES).expect("Failed to load embedded font");
        draw_text_mut(&mut err_img, Luma([0]), 10, 10, PxScale::from(16.0), &font, "LOG ] ..ERROR.. !image requires a path");
        return err_img;
    }

    let path = args[0];
    let margin = Margin::parse(&args[1..]);

    let canvas_width = (CONFIG.get().unwrap()).template.width;
    let available_width = canvas_width.saturating_sub(margin.left + margin.right).max(1);

    match image::open(path) {
        Ok(dyn_img) => {
            let mut gray = dyn_img.to_luma8();
            if gray.width() > available_width {
                let ratio = available_width as f32 / gray.width() as f32;
                let new_height = ((gray.height() as f32 * ratio).round() as u32).max(1);
                gray = image::imageops::resize(&gray, available_width, new_height, image::imageops::FilterType::Lanczos3);
            }
            dither(&mut gray, &BiLevel);

            let total_height = margin.top + gray.height() + margin.bottom;
            let mut result = GrayImage::from_pixel(canvas_width, total_height, Luma([255]));
            overlay(&mut result, &gray, margin.left as i64, margin.top as i64);
            result
        }
        Err(e) => {
            eprintln!("LOG ] Failed to load image '{}': {}", path, e);
            let box_height = 40u32;
            let total_height = margin.top + box_height + margin.bottom;
            let mut err_img = GrayImage::from_pixel(canvas_width, total_height, Luma([255]));
            let font = FontRef::try_from_slice(UNIFONT_BYTES).expect("LOG ] Failed to load embedded font");
            let box_width = available_width.max(100);
            draw_hollow_rect_mut(
                &mut err_img,
                Rect::at(margin.left as i32, margin.top as i32).of_size(box_width, box_height),
                Luma([0]),
            );
            draw_text_mut(
                &mut err_img,
                Luma([0]),
                (margin.left + 15) as i32,
                (margin.top + 12) as i32,
                PxScale::from(16.0),
                &font,
                &format!("IMAGE NOT FOUND: {}", path),
            );
            err_img
        }
    }
}

// --- Text Macros with Parameter Support ---

// Returns date/time. Optional arg[0]: strftime format (e.g. "%Y-%m-%d %H:%M" or "%A, %B %d").
fn get_date(args: &[&str]) -> String {
    let now = Zoned::now()
        .round(Unit::Second)
        .expect("LOG ] Error at get_date()");
    if !args.is_empty() {
        let fmt = args[0];
        now.strftime(fmt).to_string()
    } else {
        now.to_string()
    }
}

// Returns configured profile location string.
fn get_location(_args: &[&str]) -> String {
    CONFIG.get().map(|c| c.profile.location.clone()).unwrap_or_default()
}

fn read_toml(toml_input_fs: &str) -> Config {
    let file_content = fs::read_to_string(toml_input_fs).expect("LOG ] Could not read TOML");
    toml::from_str(&file_content).expect("LOG ] Failed to parse TOML")
}

// Splits tag content into command and arguments, respecting quotes.
fn parse_tag_parts(input: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = ' ';

    for c in input.chars() {
        if in_quotes {
            if c == quote_char {
                in_quotes = false;
            } else {
                current.push(c);
            }
        } else if c == '"' || c == '\'' {
            in_quotes = true;
            quote_char = c;
        } else if c.is_whitespace() {
            if !current.is_empty() {
                parts.push(current.clone());
                current.clear();
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

// --- Engine: tokenize() -> draw_image() -> print_image()
fn tokenize(
    input: &str,
    image_macros: &HashMap<&str, ImageMacroFn>,
    text_macros: &HashMap<&str, TextMacroFn>,
) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut remaining = input;

    while let Some(start_idx) = remaining.find("{{") {
        if start_idx > 0 {
            tokens.push(Token::Text(remaining[..start_idx].to_string()));
        }

        let after_open = &remaining[start_idx + 2..];
        if let Some(end_offset) = after_open.find("}}") {
            let tag_content = after_open[..end_offset].trim();
            let parts = parse_tag_parts(tag_content);

            if parts.is_empty() {
                tokens.push(Token::Text(format!("{{{{ {} }}}}", tag_content)));
            } else {
                let tag_name = parts[0].as_str();
                let tag_args: Vec<&str> = parts[1..].iter().map(|s| s.as_str()).collect();

                if tag_name.starts_with('!') {
                    // Call image macro with arguments
                    if let Some(macro_func) = image_macros.get(tag_name) {
                        tokens.push(Token::DynamicImage(macro_func(&tag_args)));
                        println!("LOG ] Image macro \"{}\" called with the following args: \"{}\"", tag_name.to_uppercase(), tag_args.join(" "));
                    } else {
                        tokens.push(Token::Text(format!("{{{{ {} }}}}", tag_content)));
                    }
                } else if tag_name.starts_with('#') {
                    // Call text macro with arguments
                    if let Some(macro_func) = text_macros.get(tag_name) {
                        tokens.push(Token::Text(macro_func(&tag_args)));
                        println!("LOG ] Text macro \"{}\" called with the following args: \"{}\"", tag_name.to_uppercase(), tag_args.join(" "));
                    } else {
                        tokens.push(Token::Text(format!("{{{{ {} }}}}", tag_content)));
                    }
                } else {
                    match tag_name {
                        "bold" => tokens.push(Token::Bold(true)),
                        "\\bold" => tokens.push(Token::Bold(false)),
                        "emph" => tokens.push(Token::Italic(true)),
                        "\\emph" => tokens.push(Token::Italic(false)),
                        "underline" => tokens.push(Token::Underline(true)),
                        "\\underline" => tokens.push(Token::Underline(false)),
                        "rev" | "invert" => tokens.push(Token::Reverse(true)),
                        "\\rev" | "\\invert" => tokens.push(Token::Reverse(false)),
                        "height" => {
                            if let Some(val) = tag_args.first().and_then(|v| v.parse::<usize>().ok()) {
                                tokens.push(Token::Height(val));
                            } else {
                                tokens.push(Token::Text(format!("{{{{ {} }}}}", tag_content)));
                            }
                        }
                        "width" => {
                            if let Some(val) = tag_args.first().and_then(|v| v.parse::<usize>().ok()) {
                                tokens.push(Token::Width(val));
                            } else {
                                tokens.push(Token::Text(format!("{{{{ {} }}}}", tag_content)));
                            }
                        }
                        "scale" => {
                            if let Some(val) = tag_args.first().and_then(|v| v.parse::<usize>().ok()) {
                                tokens.push(Token::Width(val));
                                tokens.push(Token::Height(val));
                            } else {
                                tokens.push(Token::Text(format!("{{{{ {} }}}}", tag_content)));
                            }
                        }
                        "vspace" => {
                            let px = tag_args.first().and_then(|v| v.parse::<u32>().ok()).unwrap_or(16);
                            tokens.push(Token::VSpace(px));
                        }
                        "hr" | "rule" | "line" => {
                            let thickness = tag_args.first().and_then(|v| v.parse::<u32>().ok()).unwrap_or(1);
                            tokens.push(Token::HLine(thickness));
                        }
                        _ => {
                            tokens.push(Token::Text(format!("{{{{ {} }}}}", tag_content)));
                            println!("LOG ] Bad tag found: {{{{ {} }}}}, ignoring.", tag_content)
                        }
                    }
                }
            }

            remaining = &after_open[end_offset + 2..];
        } else {
            tokens.push(Token::Text(remaining.to_string()));
            break;
        }
    }

    if !remaining.is_empty() {
        tokens.push(Token::Text(remaining.to_string()));
    }

    tokens
}

// Measures the required canvas height for continuous receipt rendering.
fn measure_canvas_height(tokens: &[Token]) -> u32 {
    let mut y = 0i32;
    let mut x = 0i32;
    let mut height_multiplier = 1.0f32;
    let base_font_height = 16.0f32;

    for token in tokens {
        match token {
            Token::Height(h) => height_multiplier = *h as f32,
            Token::Text(text) => {
                for (i, line) in text.split('\n').enumerate() {
                    if i > 0 {
                        x = 0;
                        y += (base_font_height * height_multiplier) as i32;
                    }
                    if !line.is_empty() {
                        x += 1;
                    }
                }
            }
            Token::DynamicImage(img) => {
                if x > 0 {
                    x = 0;
                    y += (base_font_height * height_multiplier) as i32;
                }
                y += img.height() as i32;
            }
            Token::VSpace(px) => {
                y += *px as i32;
            }
            Token::HLine(thickness) => {
                if x > 0 {
                    x = 0;
                    y += (base_font_height * height_multiplier) as i32;
                }
                y += *thickness as i32 + 6;
            }
            _ => {}
        }
    }

    let total = y + (base_font_height * height_multiplier) as i32 + 32;
    total.max(200) as u32
}

fn draw_image(tokens: Vec<Token>, &img_x: &u32) -> GrayImage {
    let total_y = measure_canvas_height(&tokens);
    let mut img = GrayImage::from_pixel(img_x, total_y, Luma([255]));

    let base_font = FontRef::try_from_slice(UNIFONT_BYTES).expect("ERROR ] Failed to load base font!");
    let upper_font = FontRef::try_from_slice(UNIFONT_UPPER_BYTES).expect("ERROR ] Failed to load upper font!");

    let mut is_bold = false;
    let mut is_underline = false;
    let mut is_reverse = false;

    let mut width_multiplier = 1.0;
    let mut height_multiplier = 1.0;

    let mut x = 0;
    let mut y = 0;
    let base_font_width = 16.0;
    let base_font_height = 16.0;

    let black = Luma([0]);
    let white = Luma([255]);

    for token in tokens {
        match token {
            Token::Bold(b) => is_bold = b,
            Token::Underline(u) => is_underline = u,
            Token::Reverse(r) => is_reverse = r,
            Token::Width(w) => width_multiplier = w as f32,
            Token::Height(h) => height_multiplier = h as f32,
            Token::Italic(_) => {}
            Token::VSpace(px) => {
                y += px as i32;
            }
            Token::HLine(thickness) => {
                if x > 0 {
                    x = 0;
                    y += (base_font_height * height_multiplier) as i32;
                }
                for t in 0..thickness {
                    draw_line_segment_mut(
                        &mut img,
                        (0.0, (y + t as i32) as f32),
                        (img_x as f32 - 1.0, (y + t as i32) as f32),
                        black,
                    );
                }
                y += thickness as i32 + 6;
            }
            Token::Text(text) => {
                let current_scale = PxScale {
                    x: base_font_width * width_multiplier,
                    y: base_font_height * height_multiplier,
                };

                for (i, line) in text.split('\n').enumerate() {
                    if i > 0 {
                        x = 0;
                        y += (base_font_height * height_multiplier) as i32;
                    }

                    if line.is_empty() {
                        continue;
                    }

                    for c in line.chars() {
                        let active_font = if base_font.glyph_id(c).0 != 0 {
                            &base_font
                        } else if upper_font.glyph_id(c).0 != 0 {
                            &upper_font
                        } else {
                            &base_font
                        };

                        let mut char_buf = [0; 4];
                        let char_str = c.encode_utf8(&mut char_buf);

                        let (char_w, char_h) = text_size(current_scale, active_font, char_str);

                        let bg_color = if is_reverse { black } else { white };
                        let fg_color = if is_reverse { white } else { black };

                        if is_reverse {
                            draw_filled_rect_mut(
                                &mut img,
                                Rect::at(x, y).of_size(char_w, char_h),
                                bg_color,
                            );
                        }

                        draw_text_mut(
                            &mut img,
                            fg_color,
                            x,
                            y,
                            current_scale,
                            active_font,
                            char_str,
                        );

                        if is_bold {
                            draw_text_mut(
                                &mut img,
                                fg_color,
                                x + 2,
                                y,
                                current_scale,
                                active_font,
                                char_str,
                            );
                            if is_reverse {
                                draw_filled_rect_mut(
                                    &mut img,
                                    Rect::at(x + char_w as i32, y).of_size(1, char_h),
                                    bg_color,
                                );
                            }
                        }

                        if is_underline {
                            let line_y = y as f32 + char_h as f32 + 2.0;
                            draw_line_segment_mut(
                                &mut img,
                                (x as f32, line_y),
                                ((x + char_w as i32) as f32, line_y),
                                fg_color,
                            );
                        }

                        x += char_w as i32;
                    }
                }
            }
            Token::DynamicImage(luma_img) => {
                if x > 0 {
                    x = 0;
                    y += (base_font_height * height_multiplier) as i32;
                }

                overlay(&mut img, &luma_img, x as i64, y as i64);
                y += luma_img.height() as i32;
            }
        }
    }

    match save_buffer_with_format("output.png", img.as_raw(), img_x, total_y, L8, Png) {
        Ok(_) => println!("LOG ] Successfully saved output.png ({}x{} px).", img_x, total_y),
        Err(e) => println!("ERROR ] Failed to save image {}!", e),
    }

    img
}

#[allow(dead_code)]
fn print_image(img: GrayImage) -> Result<()> {
    let driver = UsbDriver::open(0x0519, 0x0001, None, None)?;
    let mut printer = Printer::new(driver, Protocol::default(), Some(PrinterOptions::default()));
    printer.init()?.print()?;

    // Give each band its own raster header and flush it before queuing the next band.
    for top in (0..img.height()).step_by(64) {
        let band_height = (img.height() - top).min(64);
        let band = image::imageops::crop_imm(&img, 0, top, img.width(), band_height).to_image();
        let mut png = Cursor::new(Vec::new());
        DynamicImage::ImageLuma8(band)
            .write_to(&mut png, Png)
            .expect("ERROR ] Failed to encode image band!");

        printer
            .bit_image_from_bytes_option(
                &png.into_inner(),
                BitImageOption::new(Some(576), None, BitImageSize::DoubleHeight)?,
            )?
            .print()?;
    }

    printer.feed()?.print_cut()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_margin_parsing() {
        assert_eq!(
            Margin::parse(&["10"]),
            Margin { top: 10, right: 10, bottom: 10, left: 10 }
        );
        assert_eq!(
            Margin::parse(&["10", "20"]),
            Margin { top: 10, right: 20, bottom: 10, left: 20 }
        );
        assert_eq!(
            Margin::parse(&["10", "20", "30"]),
            Margin { top: 10, right: 20, bottom: 30, left: 20 }
        );
        assert_eq!(
            Margin::parse(&["10", "20", "30", "40"]),
            Margin { top: 10, right: 20, bottom: 30, left: 40 }
        );
        assert_eq!(
            Margin::parse(&["10px", "20px", "30px", "40px"]),
            Margin { top: 10, right: 20, bottom: 30, left: 40 }
        );
        assert_eq!(
            Margin::parse(&["10 20 30 40"]),
            Margin { top: 10, right: 20, bottom: 30, left: 40 }
        );
    }
}
