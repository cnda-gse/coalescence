use ab_glyph::{FontRef, PxScale};
use image::{GrayImage, Luma};
use imageproc::drawing::{
    draw_filled_rect_mut, draw_hollow_rect_mut, draw_line_segment_mut, draw_text_mut,
};
use imageproc::rect::Rect;
use serde::Deserialize;
use std::error::Error;
use std::time::Duration;
use crate::UNIFONT_BYTES;

// Define canvas dimensions
const WIDTH: i32 = 512;
const HEIGHT: i32 = 700;

#[derive(Debug, Deserialize)]
struct OpenMeteoWeatherResponse {
    latitude: f64,
    longitude: f64,
    hourly: HourlyData,
    daily: DailyData,
}

#[derive(Debug, Deserialize)]
struct OpenMeteoGeocodeResponse {
    results: Option<Vec<GeocodeResult>>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct GeocodeResult {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub timezone: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct HourlyData {
    time: Vec<String>,
    temperature_2m: Vec<f32>,
    surface_pressure: Vec<f32>,
    relative_humidity_2m: Vec<f32>,
    precipitation_probability: Vec<u8>,
    precipitation: Vec<f32>,
    weather_code: Vec<u8>,
    wind_speed_10m: Vec<f32>,
    wind_direction_10m: Vec<u16>,
    wind_gusts_10m: Vec<f32>,
}

#[derive(Debug, Deserialize)]
struct DailyData {
    time: Vec<String>,
    temperature_2m_max: Vec<f32>,
    temperature_2m_min: Vec<f32>,
    precipitation_probability_max: Vec<u8>,
    precipitation_sum: Vec<f32>,
    wind_speed_10m_max: Vec<f32>,
    wind_direction_10m_dominant: Vec<u16>,
    wind_gusts_10m_max: Vec<f32>,
    weather_code: Vec<u8>,
}

pub fn fetch_location(location: &str) -> Result<GeocodeResult, Box<dyn Error>> {
    let url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=en&format=json",
        location
    );

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let response = client.get(&url).send()?.json::<OpenMeteoGeocodeResponse>()?;

    match response.results.and_then(|r| r.into_iter().next()) {
        Some(loc) => Ok(loc),
        None => Err(format!("Could not find coordinates for: {}", location).into()),
    }
}

fn fetch_weather(lat: f64, lon: f64) -> Result<OpenMeteoWeatherResponse, Box<dyn Error>> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}\
        &hourly=temperature_2m,relative_humidity_2m,precipitation_probability,precipitation,weather_code,surface_pressure,wind_speed_10m,wind_direction_10m,wind_gusts_10m,cloud_cover\
        &daily=weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max,precipitation_sum,wind_speed_10m_max,wind_direction_10m_dominant,wind_gusts_10m_max\
        &timezone=auto\
        &forecast_days=6",
        lat, lon
    );
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let response = client.get(&url).send()?.json::<OpenMeteoWeatherResponse>()?;
    Ok(response)
}

fn wind_dir_arrow(degrees: u16) -> &'static str {
    let val = ((degrees as f32 / 45.0) + 0.5).floor() as u16 % 8;
    match val {
        0 => "↓",  // N (blowing South)
        1 => "↙",  // NE
        2 => "←",  // E
        3 => "↖",  // SE
        4 => "↑",  // S
        5 => "↗",  // SW
        6 => "→",  // W
        7 => "↘",  // NW
        _ => "?",
    }
}

fn wmo_icon(code: u8) -> &'static str {
    match code {
        0 => "🌣",
        1 | 2 => "⛅",
        3 => "☁",
        45 | 48 => "🌫",
        51..=67 | 80..=82 => "🌧",
        71..=77 | 85..=86 => "❄",
        95..=99 => "⛈",
        _ => "☁",
    }
}

pub fn generate_with_coords(lat: f64, lon: f64, location_label: &str) -> GrayImage {
    let weather_data = match fetch_weather(lat, lon) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("LOG ] Failed to fetch weather data: {}", e);
            let mut err_img = GrayImage::from_pixel(WIDTH as u32, 100, Luma([255]));
            let font = FontRef::try_from_slice(UNIFONT_BYTES).expect("Failed to load embedded font");
            draw_text_mut(&mut err_img, Luma([0]), 10, 30, PxScale::from(16.0), &font, "WEATHER DATA UNAVAILABLE (OFFLINE)");
            return err_img;
        }
    };

    let mut img = GrayImage::from_pixel(WIDTH as u32, HEIGHT as u32, Luma([255]));
    let black = Luma([0u8]);

    let font = FontRef::try_from_slice(UNIFONT_BYTES).expect("Failed to load embedded font");
    let scale_norm = PxScale::from(16.0);

    draw_text_mut(
        &mut img,
        black,
        0,
        0,
        scale_norm,
        &font,
        &format!("LOC \"{}\": {:.2}N, {:.2}W", location_label, weather_data.latitude, weather_data.longitude.abs()),
    );
    let mut y_offset = 15;
    draw_text_mut(&mut img, black, 0, y_offset, scale_norm, &font, "┌──────────────────────── 24H OUTLOOK ─────────────────────────┐");
    y_offset += 60;

    // 2. Data Extraction for 24-Hour Graph (8 intervals, every 3 hours)
    let hourly_indices: Vec<usize> = (0..8).map(|i| i * 3).collect();

    let graph_x_start = 120i32;
    let graph_y_start = y_offset;
    let graph_width = WIDTH - graph_x_start;
    let graph_height = 160;
    let col_spacing = graph_width / (hourly_indices.len() as i32);

    let max_temp = hourly_indices.iter().map(|&i| weather_data.hourly.temperature_2m[i]).fold(f32::MIN, f32::max) + 2.0;

    let mut prev_point_temp: Option<(f32, f32)> = None;
    let mut prev_point_humid: Option<(f32, f32)> = None;

    for (col, &i) in hourly_indices.iter().enumerate() {
        let x = graph_x_start + (col as i32 * col_spacing) + (col_spacing / 2);

        // Parse Time (e.g. "2026-10-12T17:00" -> "17h")
        let raw_time = &weather_data.hourly.time[i];
        let hour_str = raw_time.split('T').nth(1).unwrap_or("00:00").split(':').next().unwrap_or("00");
        let formatted_time = format!("{}h", hour_str);

        draw_text_mut(&mut img, black, x - 12, graph_y_start - 30, scale_norm, &font, &formatted_time);
        draw_line_segment_mut(&mut img, (x as f32, graph_y_start as f32 - 5.0), (x as f32, (graph_y_start + graph_height) as f32), black);

        // Precip Probability Bars
        let precip_val = weather_data.hourly.precipitation_probability[i] as f32;
        let bar_height = ((precip_val / 100.0) * graph_height as f32) as i32;
        if bar_height > 0 {
            let rect = Rect::at(x - 10, graph_y_start + graph_height - bar_height).of_size(20, bar_height as u32);
            draw_hollow_rect_mut(&mut img, rect, black);
        }

        // Temp + Humidity Line Graph
        let temp_val = weather_data.hourly.temperature_2m[i];
        let temp_y = graph_y_start + graph_height - (((temp_val) / (max_temp.max(1.0))) * graph_height as f32) as i32;

        let humid_val = weather_data.hourly.relative_humidity_2m[i];
        let humid_y = graph_y_start + graph_height - (((humid_val) / 100.0) * graph_height as f32) as i32;

        if let Some((px, py)) = prev_point_temp {
            draw_line_segment_mut(&mut img, (px, py), (x as f32, temp_y as f32), black);
            if col == 1 {
                draw_text_mut(&mut img, black, graph_x_start - 20, temp_y - 10, scale_norm, &font, "TEMP");
            }
        }

        if let Some((px, py)) = prev_point_humid {
            draw_line_segment_mut(&mut img, (px, py), (x as f32, humid_y as f32), black);
            if col == 1 {
                draw_text_mut(&mut img, black, graph_x_start - 20, humid_y, scale_norm, &font, "HUMD");
            }
        }

        draw_filled_rect_mut(&mut img, Rect::at(x - 4, temp_y - 4).of_size(8, 8), black);
        draw_hollow_rect_mut(&mut img, Rect::at(x - 4, humid_y - 4).of_size(8, 8), black);

        prev_point_temp = Some((x as f32, temp_y as f32));
        prev_point_humid = Some((x as f32, humid_y as f32));
    }

    // Baseline
    draw_line_segment_mut(&mut img, (graph_x_start as f32, (graph_y_start + graph_height) as f32), (492.0, (graph_y_start + graph_height) as f32), black);

    // 3. 24-Hour Data Table (Dynamic Population)
    let table_headers = ["🌡 TEMP[°C]", "⛆ PRCP[%]", "🥤 AMNT[mm]", "🌢 HUMD[%]", "㍱ PRES[hPa]", "🌫 WIND[km/h]", "🏳 GUST[km/h]"];
    let mut table_data: Vec<Vec<String>> = vec![vec![]; 7];

    for &i in &hourly_indices {
        table_data[0].push(format!("{:.0}", weather_data.hourly.temperature_2m[i]));
        table_data[1].push(format!("{:02}", weather_data.hourly.precipitation_probability[i]));
        table_data[2].push(format!("{:.1}", weather_data.hourly.precipitation[i]));
        table_data[3].push(format!("{:.0}", weather_data.hourly.relative_humidity_2m[i]));
        table_data[4].push(format!("{:.0}", weather_data.hourly.surface_pressure[i]));
        table_data[5].push(format!("{}{:.0}", wind_dir_arrow(weather_data.hourly.wind_direction_10m[i]), weather_data.hourly.wind_speed_10m[i]));
        table_data[6].push(format!("G{:.0}", weather_data.hourly.wind_gusts_10m[i]));
    }

    y_offset = graph_y_start + graph_height + 10;
    for (r, row_label) in table_headers.iter().enumerate() {
        draw_text_mut(&mut img, black, 10, y_offset, scale_norm, &font, row_label);
        for (c, val) in table_data[r].iter().enumerate() {
            let x = graph_x_start + (c as i32 * col_spacing) + (col_spacing / 2) - 10;
            draw_text_mut(&mut img, black, x, y_offset, scale_norm, &font, val);
        }
        y_offset += 24;
    }

    // 4. 5-Day Outlook (Tabulation)
    y_offset += 20;
    draw_text_mut(&mut img, black, 0, y_offset, scale_norm, &font, "├──────────────────────── 5DY OUTLOOK ─────────────────────────┤");
    y_offset += 30;

    let daily_headers = [
        "📅 DATE",
        "🌣 COND",
        "▲ MAX[°C]",
        "▼ MIN[°C]",
        "⛆ PRCP[%]",
        "🥤 AMNT[mm]",
        "🌫 WIND[km/h]",
        "🏳 GUST[km/h]",
    ];
    let mut daily_data: Vec<Vec<String>> = vec![vec![]; 8];
    let daily_col_spacing = (WIDTH - graph_x_start) / 5;

    for i in 1..=5 {
        let date_str = &weather_data.daily.time[i][5..]; // Extracts "MM-DD" from "YYYY-MM-DD"
        let icon_str = wmo_icon(weather_data.daily.weather_code[i]);
        let max_t = format!("{:.0}", weather_data.daily.temperature_2m_max[i]);
        let min_t = format!("{:.0}", weather_data.daily.temperature_2m_min[i]);
        let prcp_p = format!("{:02}", weather_data.daily.precipitation_probability_max[i]);
        let prcp_s = format!("{:.1}", weather_data.daily.precipitation_sum[i]);
        let wind_s = format!("{}{:.0}", wind_dir_arrow(weather_data.daily.wind_direction_10m_dominant[i]), weather_data.daily.wind_speed_10m_max[i]);
        let gust_s = format!("G{:.0}", weather_data.daily.wind_gusts_10m_max[i]);

        daily_data[0].push(date_str.to_string());
        daily_data[1].push(icon_str.to_string());
        daily_data[2].push(max_t);
        daily_data[3].push(min_t);
        daily_data[4].push(prcp_p);
        daily_data[5].push(prcp_s);
        daily_data[6].push(wind_s);
        daily_data[7].push(gust_s);
    }

    for (r, row_label) in daily_headers.iter().enumerate() {
        draw_text_mut(&mut img, black, 10, y_offset, scale_norm, &font, row_label);
        for (c, val) in daily_data[r].iter().enumerate() {
            let x = graph_x_start + (c as i32 * daily_col_spacing) + (daily_col_spacing / 2) - 10;
            draw_text_mut(&mut img, black, x, y_offset, scale_norm, &font, val);
        }
        y_offset += 24;
    }

    y_offset += 10;
    draw_text_mut(&mut img, black, 0, y_offset, scale_norm, &font, "└──────────────────────────── END ─────────────────────────────┘");

    img
}

pub fn generate(search_string: String) -> GrayImage {
    let location = match fetch_location(&search_string) {
        Ok(location) => location,
        Err(e) => {
            eprintln!("LOG ] Failed to geocode coordinates: {}", e);
            let mut img = GrayImage::from_pixel(WIDTH as u32, 100, Luma([255]));
            let font = FontRef::try_from_slice(UNIFONT_BYTES).expect("Failed to load embedded font");
            draw_text_mut(&mut img, Luma([0]), 10, 30, PxScale::from(16.0), &font, "LOCATION LOOKUP FAILED");
            return img;
        }
    };

    generate_with_coords(location.latitude, location.longitude, &search_string)
}
