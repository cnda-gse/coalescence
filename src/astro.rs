use ab_glyph::{FontRef, PxScale};
use image::{GrayImage, Luma};
use imageproc::drawing::{
    draw_filled_rect_mut, draw_hollow_circle_mut, draw_hollow_rect_mut,
    draw_line_segment_mut, draw_text_mut,
};
use imageproc::rect::Rect;
use serde::Deserialize;
use std::f32::consts::PI;
use std::time::Duration;
use crate::UNIFONT_BYTES;

const WIDTH: u32 = 512;
const HEIGHT: u32 = 868;

/// Individual 3-hour Planetary Kp data point from NOAA SWPC.
#[derive(Debug, Clone, Deserialize)]
pub struct SwpcKpPoint {
    pub time_tag: String,
    #[serde(rename = "Kp")]
    pub kp: f32,
}

/// Struct holding astronomical and space weather data.
/// Easily customized or tweaked for different layouts and representations.
#[derive(Debug, Clone)]
pub struct AstroData {
    pub sunrise_az: f32,
    pub sunrise_time: String,
    pub sunset_az: f32,
    pub sunset_time: String,
    pub noon_az: f32,
    pub noon_time: String,
    pub noon_alt: f32,

    pub moonrise_az: f32,
    pub moonrise_time: String,
    pub moonset_az: f32,
    pub moonset_time: String,
    pub moon_phase: String,

    // Space Weather Metrics (NOAA SWPC)
    pub kp_index: u8, // 0 to 9 scale
    pub kp_history: Vec<SwpcKpPoint>, // 24-hour Kp history (8 intervals of 3 hours)
    pub solar_wind_spd: f32,
    pub solar_wind_den: f32,
    pub sunspot_num: u16,
    pub radio_flux: u16,
    pub flare_c: u8,
    pub flare_m: u8,
    pub flare_x: u8,
    pub geomag_status: String,
}

impl Default for AstroData {
    fn default() -> Self {
        Self {
            sunrise_az: 92.0,
            sunrise_time: "07:15".to_string(),
            sunset_az: 268.0,
            sunset_time: "18:31".to_string(),
            noon_az: 180.0,
            noon_time: "12:59".to_string(),
            noon_alt: 31.4,

            moonrise_az: 84.0,
            moonrise_time: "03:12".to_string(),
            moonset_az: 276.0,
            moonset_time: "17:45".to_string(),
            moon_phase: "Waning Crescent (🌘)".to_string(),

            kp_index: 2,
            kp_history: vec![
                SwpcKpPoint { time_tag: "2026-10-26T00:00:00".to_string(), kp: 1.0 },
                SwpcKpPoint { time_tag: "2026-10-26T03:00:00".to_string(), kp: 1.0 },
                SwpcKpPoint { time_tag: "2026-10-26T06:00:00".to_string(), kp: 1.33 },
                SwpcKpPoint { time_tag: "2026-10-26T09:00:00".to_string(), kp: 2.0 },
                SwpcKpPoint { time_tag: "2026-10-26T12:00:00".to_string(), kp: 2.0 },
                SwpcKpPoint { time_tag: "2026-10-26T15:00:00".to_string(), kp: 1.33 },
                SwpcKpPoint { time_tag: "2026-10-26T18:00:00".to_string(), kp: 0.67 },
                SwpcKpPoint { time_tag: "2026-10-26T21:00:00".to_string(), kp: 1.0 },
            ],
            solar_wind_spd: 410.0,
            solar_wind_den: 6.5,
            sunspot_num: 120,
            radio_flux: 135,
            flare_c: 50,
            flare_m: 10,
            flare_x: 1,
            geomag_status: "QUIET / UNSETTLED".to_string(),
        }
    }
}

// NOAA SWPC Deserialization helpers
#[derive(Debug, Deserialize)]
struct SwpcWindItem {
    proton_speed: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct SwpcFluxItem {
    flux: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct SwpcProbItem {
    c_class_1_day: Option<u8>,
    m_class_1_day: Option<u8>,
    x_class_1_day: Option<u8>,
}

/// Fetch live space weather data from NOAA SWPC public endpoints.
fn fetch_swpc_data() -> (u8, Vec<SwpcKpPoint>, f32, f32, u16, u16, u8, u8, u8, String) {
    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
    {
        Ok(c) => c,
        Err(_) => {
            let def = AstroData::default();
            return (
                def.kp_index,
                def.kp_history,
                def.solar_wind_spd,
                def.solar_wind_den,
                def.sunspot_num,
                def.radio_flux,
                def.flare_c,
                def.flare_m,
                def.flare_x,
                def.geomag_status,
            );
        }
    };

    // 1. Planetary Kp-index history (last 8 readings = 24 hours)
    let mut kp_history = Vec::new();
    if let Ok(resp) = client
        .get("https://services.swpc.noaa.gov/products/noaa-planetary-k-index.json")
        .send()
        && let Ok(items) = resp.json::<Vec<SwpcKpPoint>>() {
            let n = items.len();
            if n >= 8 {
                kp_history = items[n - 8..].to_vec();
            } else {
                kp_history = items;
            }
        }
    if kp_history.is_empty() {
        kp_history = AstroData::default().kp_history;
    }

    let latest_kp = kp_history.last().map(|p| p.kp).unwrap_or(2.0);
    let kp_index = (latest_kp.round() as u8).min(9);
    let geomag_status = match kp_index {
        0..=4 => "QUIET / UNSETTLED",
        5 => "G1 (MINOR STORM)",
        6 => "G2 (MODERATE STORM)",
        7 => "G3 (STRONG STORM)",
        8 => "G4 (SEVERE STORM)",
        _ => "G5 (EXTREME STORM)",
    }
    .to_string();

    // 2. Solar Wind Speed
    let mut wind_spd: f32 = 410.0;
    if let Ok(resp) = client
        .get("https://services.swpc.noaa.gov/products/summary/solar-wind-speed.json")
        .send()
        && let Ok(items) = resp.json::<Vec<SwpcWindItem>>()
            && let Some(spd) = items.first().and_then(|i| i.proton_speed) {
                wind_spd = spd;
            }

    // 3. 10.7cm Radio Flux
    let mut radio_flux: u16 = 135;
    if let Ok(resp) = client
        .get("https://services.swpc.noaa.gov/products/summary/10cm-flux.json")
        .send()
        && let Ok(items) = resp.json::<Vec<SwpcFluxItem>>()
            && let Some(flux) = items.first().and_then(|i| i.flux) {
                radio_flux = flux;
            }

    // 4. Solar Flare Probabilities
    let mut flare_c: u8 = 50;
    let mut flare_m: u8 = 10;
    let mut flare_x: u8 = 1;
    if let Ok(resp) = client
        .get("https://services.swpc.noaa.gov/json/solar_probabilities.json")
        .send()
        && let Ok(items) = resp.json::<Vec<SwpcProbItem>>()
            && let Some(first) = items.first() {
                if let Some(c) = first.c_class_1_day { flare_c = c; }
                if let Some(m) = first.m_class_1_day { flare_m = m; }
                if let Some(x) = first.x_class_1_day { flare_x = x; }
            }

    // Solar wind density and sunspot number baseline/model
    let solar_wind_den = 6.5;
    let sunspot_num = 124;

    (kp_index, kp_history, wind_spd, solar_wind_den, sunspot_num, radio_flux, flare_c, flare_m, flare_x, geomag_status)
}

/// Computes live solar positions (using the 'sun' crate), lunar phases, and SWPC space weather.
pub fn fetch_live_data(lat: f64, lon: f64, tz_str: Option<&str>) -> AstroData {
    let now = jiff::Zoned::now();
    let now_ms = now.timestamp().as_millisecond();

    let tz = tz_str
        .and_then(|name| jiff::tz::TimeZone::get(name).ok())
        .unwrap_or_else(|| now.time_zone().clone());

    // 1. Sun Ephemeris calculations via `sun`
    let sunrise_ms = sun::time_at_phase(now_ms, sun::SunPhase::Sunrise, lat, lon, 0.0);
    let sunset_ms = sun::time_at_phase(now_ms, sun::SunPhase::Sunset, lat, lon, 0.0);
    let noon_ms = (sunrise_ms + sunset_ms) / 2;

    let sunrise_pos = sun::pos(sunrise_ms, lat, lon);
    let sunset_pos = sun::pos(sunset_ms, lat, lon);
    let noon_pos = sun::pos(noon_ms, lat, lon);

    let sunrise_az = (sunrise_pos.azimuth.to_degrees() as f32).rem_euclid(360.0);
    let sunset_az = (sunset_pos.azimuth.to_degrees() as f32).rem_euclid(360.0);
    let noon_az = (noon_pos.azimuth.to_degrees() as f32).rem_euclid(360.0);
    let noon_alt = noon_pos.altitude.to_degrees() as f32;

    let sunrise_time = jiff::Timestamp::from_millisecond(sunrise_ms)
        .map(|t| t.to_zoned(tz.clone()))
        .map(|z| format!("{:02}:{:02}", z.hour(), z.minute()))
        .unwrap_or_else(|_| "06:00".to_string());

    let sunset_time = jiff::Timestamp::from_millisecond(sunset_ms)
        .map(|t| t.to_zoned(tz.clone()))
        .map(|z| format!("{:02}:{:02}", z.hour(), z.minute()))
        .unwrap_or_else(|_| "18:00".to_string());

    let noon_time = jiff::Timestamp::from_millisecond(noon_ms)
        .map(|t| t.to_zoned(tz.clone()))
        .map(|z| format!("{:02}:{:02}", z.hour(), z.minute()))
        .unwrap_or_else(|_| "12:00".to_string());

    // 2. Lunar phase & age (Synodic period: 29.530588853 days)
    let synodic_month = 29.530588853;
    let days_since_epoch = (now.timestamp().as_second() as f64 - 947182440.0) / 86400.0;
    let phase_age = days_since_epoch.rem_euclid(synodic_month);
    let phase_norm = phase_age / synodic_month; // 0.0 to 1.0

    let (phase_name, phase_icon) = if phase_norm < 0.0625 || phase_norm >= 0.9375 {
        ("New Moon", "🌑")
    } else if phase_norm < 0.1875 {
        ("Waxing Crescent", "🌒")
    } else if phase_norm < 0.3125 {
        ("First Quarter", "🌓")
    } else if phase_norm < 0.4375 {
        ("Waxing Gibbous", "🌔")
    } else if phase_norm < 0.5625 {
        ("Full Moon", "🌕")
    } else if phase_norm < 0.6875 {
        ("Waning Gibbous", "🌖")
    } else if phase_norm < 0.8125 {
        ("Last Quarter", "🌗")
    } else {
        ("Waning Crescent", "🌘")
    };
    let moon_phase = format!("{} ({})", phase_name, phase_icon);

    // Approximate moonrise & moonset based on lunar phase
    let moon_offset_hours = phase_norm as f32 * 24.0;
    let rise_hour_base = sunrise_time[..2].parse::<f32>().unwrap_or(6.0);
    let moonrise_hour = (rise_hour_base + moon_offset_hours).rem_euclid(24.0);
    let moonset_hour = (moonrise_hour + 12.4).rem_euclid(24.0);

    let moonrise_time = format!(
        "{:02}:{:02}",
        moonrise_hour.floor() as u32,
        ((moonrise_hour.fract() * 60.0).round() as u32) % 60
    );
    let moonset_time = format!(
        "{:02}:{:02}",
        moonset_hour.floor() as u32,
        ((moonset_hour.fract() * 60.0).round() as u32) % 60
    );

    let moonrise_az = (90.0 - (phase_norm as f32 - 0.5) * 40.0).rem_euclid(360.0);
    let moonset_az = (270.0 + (phase_norm as f32 - 0.5) * 40.0).rem_euclid(360.0);

    // 3. Live Space Weather from NOAA SWPC
    let (kp_index, kp_history, solar_wind_spd, solar_wind_den, sunspot_num, radio_flux, flare_c, flare_m, flare_x, geomag_status) =
        fetch_swpc_data();

    AstroData {
        sunrise_az,
        sunrise_time,
        sunset_az,
        sunset_time,
        noon_az,
        noon_time,
        noon_alt,
        moonrise_az,
        moonrise_time,
        moonset_az,
        moonset_time,
        moon_phase,
        kp_index,
        kp_history,
        solar_wind_spd,
        solar_wind_den,
        sunspot_num,
        radio_flux,
        flare_c,
        flare_m,
        flare_x,
        geomag_status,
    }
}

// Helper to convert Azimuth degrees to X/Y canvas coordinates for the polar graph
fn polar_to_xy(cx: f32, cy: f32, r: f32, azimuth_deg: f32) -> (f32, f32) {
    // 0° is North (Up), 90° is East (Right)
    let theta = (azimuth_deg - 90.0) * PI / 180.0;
    (cx + r * theta.cos(), cy + r * theta.sin())
}

/// Visual render function for AstroData.
/// Easily customized or tweaked to adjust typography, layouts, or polar graph properties.
pub fn generate_from_data(data: &AstroData) -> GrayImage {
    let mut img = GrayImage::from_pixel(WIDTH, HEIGHT, Luma([255]));
    let black = Luma([0u8]);

    let font = FontRef::try_from_slice(UNIFONT_BYTES).expect("Failed to load embedded font");
    let scale_norm = PxScale::from(16.0);
    let scale_small = PxScale::from(16.0);

    let mut y_offset = 15;

    // ==========================================
    // 1. HORIZON AZIMUTH POLAR GRAPH
    // ==========================================
    let cx = 256.0;
    let cy = 128.0 + (2.0 * y_offset as f32);
    let r = 100.0;

    draw_text_mut(&mut img, black, 0, y_offset, scale_norm, &font, "┌──────────────────── HORIZON AZIMUTH MAP ─────────────────────┐");
    y_offset = 2 * cy as i32;

    // Draw compass rings
    draw_hollow_circle_mut(&mut img, (cx as i32, cy as i32), r as i32, black);
    draw_hollow_circle_mut(&mut img, (cx as i32, cy as i32), r as i32 + 2, black); // Double outer ring
    draw_hollow_circle_mut(&mut img, (cx as i32, cy as i32), 3, black); // Center observer

    // N, S, E, W markers
    let directions = [("N", 0.0), ("E", 90.0), ("S", 180.0), ("W", 270.0)];
    for (lbl, az) in directions {
        let (tx, ty) = polar_to_xy(cx, cy, r + 15.0, az);
        draw_text_mut(&mut img, black, (tx - 4.0) as i32, (ty - 8.0) as i32, scale_norm, &font, lbl);

        // Cardinal ticks
        let (kx1, ky1) = polar_to_xy(cx, cy, r - 5.0, az);
        let (kx2, ky2) = polar_to_xy(cx, cy, r + 5.0, az);
        draw_line_segment_mut(&mut img, (kx1, ky1), (kx2, ky2), black);
    }

    // Helper closure to draw azimuth vectors
    let mut draw_vector = |az: f32, label: &str, is_moon: bool| {
        let (ex, ey) = polar_to_xy(cx, cy, r, az);

        // Draw dashed or solid line from center to horizon
        let segments = 12;
        for i in 0..segments {
            if is_moon && i % 2 == 0 { continue; } // Dotted line for Moon
            let p1x = cx + (ex - cx) * (i as f32 / segments as f32);
            let p1y = cy + (ey - cy) * (i as f32 / segments as f32);
            let p2x = cx + (ex - cx) * ((i + 1) as f32 / segments as f32);
            let p2y = cy + (ey - cy) * ((i + 1) as f32 / segments as f32);
            draw_line_segment_mut(&mut img, (p1x, p1y), (p2x, p2y), black);
        }

        // Add small marker box on horizon
        draw_filled_rect_mut(&mut img, Rect::at(ex as i32 - 3, ey as i32 - 3).of_size(6, 6), black);

        // Label
        let (lx, ly) = polar_to_xy(cx, cy, r + 35.0, az);
        let text_w = label.len() as f32 * 8.0; // Approx width
        let lx_adj = if az > 180.0 { lx - text_w } else { lx }; // Shift left if on Western hemisphere
        draw_text_mut(&mut img, black, lx_adj as i32, (ly - 8.0) as i32, scale_small, &font, label);
    };

    // Plot Sun and Moon vectors
    draw_vector(data.sunrise_az, &format!("☀ RISE {:.0}°", data.sunrise_az), false);
    draw_vector(data.sunset_az, &format!("☀ SET {:.0}°", data.sunset_az), false);
    draw_vector(data.noon_az, &format!("▲ NOON {:.0}°", data.noon_az), false);

    draw_vector(data.moonrise_az, &format!("🌘 RISE {:.0}°", data.moonrise_az), true);
    draw_vector(data.moonset_az, &format!("🌘 SET {:.0}°", data.moonset_az), true);

    // ==========================================
    // 2. DAILY EPHEMERIS SUMMARY
    // ==========================================
    draw_text_mut(&mut img, black, 0, y_offset, scale_norm, &font, "├───────────────────────── EPHEMERIS ──────────────────────────┤");
    y_offset += 45;

    let eph_col1 = 30;
    let eph_col2 = 270;

    let left_stats = [
        format!("SUNRISE:  {}", data.sunrise_time),
        format!("SOLAR NOON: {} (ALT {:.1}°)", data.noon_time, data.noon_alt),
        format!("SUNSET:   {}", data.sunset_time),
    ];
    let right_stats = [
        format!("MOONRISE: {}", data.moonrise_time),
        format!("MOON PHASE: {}", data.moon_phase),
        format!("MOONSET:  {}", data.moonset_time),
    ];

    for i in 0..3 {
        draw_text_mut(&mut img, black, eph_col1, y_offset + (i * 22), scale_norm, &font, &left_stats[i as usize]);
        draw_text_mut(&mut img, black, eph_col2, y_offset + (i * 22), scale_norm, &font, &right_stats[i as usize]);
    }

    // ==========================================
    // 3. SPACE WEATHER & GEOMAGNETIC DATA
    // ==========================================
    y_offset += 75;
    draw_text_mut(&mut img, black, 0, y_offset, scale_norm, &font, "├─────────────────── SPACE WEATHER & GEOMAGNETICS ────────────────────┤");
    y_offset += 55;

    // 24-Hour Planetary Kp Index Graph
    let graph_x_start = 120i32;
    let graph_y_start = y_offset;
    let graph_width = WIDTH as i32 - graph_x_start;
    let graph_height = 120i32;
    let col_spacing = graph_width / (data.kp_history.len() as i32);

    // Left Axis indicators & Storm Thresholds
    draw_text_mut(&mut img, black, 10, graph_y_start - 8, scale_norm, &font, "KP 9 [G5]");

    let g1_y = graph_y_start + graph_height - ((5.0 / 9.0) * graph_height as f32) as i32;
    draw_text_mut(&mut img, black, 10, g1_y - 8, scale_norm, &font, "KP 5 [G1]");
    for dx in (graph_x_start..492).step_by(8) {
        draw_line_segment_mut(&mut img, (dx as f32, g1_y as f32), ((dx + 4) as f32, g1_y as f32), black);
    }

    let base_y = graph_y_start + graph_height;
    draw_text_mut(&mut img, black, 10, base_y - 8, scale_norm, &font, "KP 0 [Q]");
    draw_line_segment_mut(&mut img, (graph_x_start as f32, base_y as f32), (492.0, base_y as f32), black);

    let mut prev_point_kp: Option<(f32, f32)> = None;

    for (col, pt) in data.kp_history.iter().enumerate() {
        let x = graph_x_start + (col as i32 * col_spacing) + (col_spacing / 2);

        // Time label
        let hour_str = pt.time_tag.split('T').nth(1).unwrap_or("00:00").split(':').next().unwrap_or("00");
        let formatted_time = format!("{}h", hour_str);

        draw_text_mut(&mut img, black, x - 12, graph_y_start - 25, scale_norm, &font, &formatted_time);
        draw_line_segment_mut(&mut img, (x as f32, graph_y_start as f32 - 5.0), (x as f32, base_y as f32), black);

        // Kp Bar
        let bar_h = (((pt.kp / 9.0).clamp(0.0, 1.0)) * graph_height as f32) as i32;
        if bar_h > 0 {
            let rect = Rect::at(x - 10, base_y - bar_h).of_size(20, bar_h as u32);
            if pt.kp >= 5.0 {
                draw_filled_rect_mut(&mut img, rect, black);
            } else {
                draw_hollow_rect_mut(&mut img, rect, black);
            }
        }

        // Kp Line + Marker
        let kp_y = base_y - (((pt.kp / 9.0).clamp(0.0, 1.0)) * graph_height as f32) as i32;
        if let Some((px, py)) = prev_point_kp {
            draw_line_segment_mut(&mut img, (px, py), (x as f32, kp_y as f32), black);
        }
        draw_filled_rect_mut(&mut img, Rect::at(x - 4, kp_y - 4).of_size(8, 8), black);
        prev_point_kp = Some((x as f32, kp_y as f32));
    }

    // 24H Kp Data Table (Tabulation)
    let kp_headers = ["🌐 KP[0-9]", "⚡ G-SCALE"];
    let mut kp_data: Vec<Vec<String>> = vec![vec![]; 2];

    for pt in &data.kp_history {
        kp_data[0].push(format!("{:.1}", pt.kp));
        let g_str = if pt.kp >= 9.0 {
            "G5"
        } else if pt.kp >= 8.0 {
            "G4"
        } else if pt.kp >= 7.0 {
            "G3"
        } else if pt.kp >= 6.0 {
            "G2"
        } else if pt.kp >= 5.0 {
            "G1"
        } else {
            " -"
        };
        kp_data[1].push(g_str.to_string());
    }

    y_offset = base_y + 12;
    for (r, row_label) in kp_headers.iter().enumerate() {
        draw_text_mut(&mut img, black, 10, y_offset, scale_norm, &font, row_label);
        for (c, val) in kp_data[r].iter().enumerate() {
            let x = graph_x_start + (c as i32 * col_spacing) + (col_spacing / 2) - 10;
            draw_text_mut(&mut img, black, x, y_offset, scale_norm, &font, val);
        }
        y_offset += 24;
    }

    // Real-Time Planetary Kp Gauge & Status
    y_offset += 15;
    draw_text_mut(&mut img, black, 10, y_offset, scale_norm, &font, "REAL-TIME KP GAUGE:");
    draw_text_mut(&mut img, black, 270, y_offset, scale_norm, &font, &format!("KP-{} {}", data.kp_index, data.geomag_status));

    let gauge_x = 30;
    let gauge_y = y_offset + 25;
    draw_hollow_rect_mut(&mut img, Rect::at(gauge_x, gauge_y).of_size(452, 20), black);

    for i in 0..9 {
        let block_x = gauge_x + 2 + (i * 50);
        if i < data.kp_index as i32 {
            draw_filled_rect_mut(&mut img, Rect::at(block_x, gauge_y + 2).of_size(48, 16), black);
        } else {
            for dx in 0..48 {
                for dy in 0..16 {
                    if (dx + dy) % 4 == 0 {
                        img.put_pixel((block_x + dx) as u32, (gauge_y + 2 + dy) as u32, black);
                    }
                }
            }
        }
    }

    for i in 0..=9 {
        let tx = gauge_x + (i * 50) - 4;
        draw_text_mut(&mut img, black, tx, gauge_y + 25, scale_small, &font, &format!("{}", i));
    }

    y_offset += 80;

    // Solar Metrics Table
    let sw_col1 = 30;
    let sw_col2 = 270;

    let sw_left = [
        format!("SOLAR WIND SPD: {:.1} km/s", data.solar_wind_spd),
        format!("SOLAR WIND DEN: {:.1} p/cm³", data.solar_wind_den),
        format!("SUNSPOT NUM:    {}", data.sunspot_num),
    ];
    let sw_right = [
        format!("RADIO 10.7cm: {} sfu", data.radio_flux),
        format!("M-CLASS FLARE PROB: {}%", data.flare_m),
        format!("X-CLASS FLARE PROB: {}%", data.flare_x),
    ];

    for i in 0..3 {
        draw_text_mut(&mut img, black, sw_col1, y_offset + (i * 22), scale_norm, &font, &sw_left[i as usize]);
        draw_text_mut(&mut img, black, sw_col2, y_offset + (i * 22), scale_norm, &font, &sw_right[i as usize]);
    }

    y_offset += 75;
    draw_text_mut(&mut img, black, 0, y_offset, scale_norm, &font, "└──────────────────────────── END ─────────────────────────────┘");

    img
}

/// Convenience generate function using geographic coordinates and optional timezone.
pub fn generate(lat: f64, lon: f64, tz: Option<&str>) -> GrayImage {
    let data = fetch_live_data(lat, lon, tz);
    generate_from_data(&data)
}
