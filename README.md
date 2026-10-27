# coalescence

**coalescence** is a modular reporting engine and 'environmental dashboard' written in Rust. It is designed to provide useful information in a compact format for everyday use.

It is designed primarily for use with ESC/POS thermal printers. However, as the software directly outputs a plain PNG, it can be stored or printed on any medium.

---

## Highlights

- **Bitmap-First Sandboxing + Safe Band Slicing**  
  - Bypasses individual printer firmware quirks, erratic hardware code pages, and missing printer fonts by rendering everything into a 1-bit bitmap in memory before printing. What you preview in `output.png` is byte-for-byte what appears on physical paper.
  - Sends image data to thermal printers in 64-row bands with dedicated raster headers, preventing hardware buffer overruns on USB and serial printers.


- **Dynamic Roll Sizing**  
  As thermal paper rolls are continuous strips, `coalescence` dynamically measures your template's vertical displacement, automatically allocating the exact canvas height needed so nothing gets truncated nor paper wasted.


- **Near-Complete Unicode Coverage**  
  Embedded with [**UnifontEX**](https://github.com/stgiga/UnifontEX) and [**Unifont Upper**](https://unifoundry.com/unifont/index.html) 18.0.01. Every box-drawing border, esoteric script, mathematical symbol, arrow, and ASCII art snippet *should* print crisply without missing glyphs.


- **Live Astrometry & Space Weather**  
  - **Solar Ephemeris via `sun`**: Calculates exact sunrise, sunset, and solar noon azimuths and altitudes for your location, rendered on an N-up polar horizon map.
  
  - **Lunar Ephemeris**: Reckons moon phase, illumination cycle, and estimated lunar rise/set vectors.

  - **NOAA SWPC Integration**: Pulls live Planetary Kp-index, 24-hour geomagnetic storm activity graph (with G1 threshold warning line), solar wind velocity, 10.7cm radio flux, and flare probabilities directly from NOAA SWPC feeds with graceful offline fallbacks.


- **OpenMeteo Weather Macro**  
  Geocodes locations automatically and renders a 24-hour temperature/humidity graph, precipitation probability histogram, dynamic 24-hour data table, and a matrix-tabulated 5-day forecast with WMO weather glyphs.

---

## Templating Syntax

Templates are written directly inside `config.toml` using a lightweight Jinja/Tera-inspired tag system:

### Text Styling
| Tag | Description |
|---|---|
| `{{ bold }}` ... `{{ \bold }}` | Bold text |
| `{{ underline }}` ... `{{ \underline }}` | Underlined text |
| `{{ rev }}` ... `{{ \rev }}` | Inverted text (white-on-black background fill) |
| `{{ emph }}` ... `{{ \emph }}` | Italic emphasis |

### Font Scaling
| Tag | Description |
|---|---|
| `{{ scale N }}` | Uniform multiplier for width and height |
| `{{ width N }}` | Multiplies character width by `N` |
| `{{ height N }}` | Multiplies character height by `N` |

### Built-in Macros & Parameters
Macro tags support optional parameters. Strings containing spaces can be enclosed in double or single quotes:

| Macro Tag | Type | Description |
|---|---|---|
| `{{ !weather }}` | Image | 24-hour chart, data table, and 5-day forecast for profile location |
| `{{ !weather "Tokyo" }}` | Image | 24-hour chart, data table, and 5-day forecast for a specific location |
| `{{ !astro }}` | Image | Polar horizon azimuth map, ephemeris summary, and 24H Kp graph for profile location |
| `{{ !astro "London" }}` | Image | Polar horizon azimuth map, ephemeris summary, and 24H Kp graph for a specific location |
| `{{ !image "path/file.png" [margin] }}` | Image | Injects image with CSS-style margins (`T R B L`, e.g. `20 40 20 40` or `"10 20"`) and dithers to 1-bit |
| `{{ #date }}` | Text | Current localized timestamp via `jiff` (ISO format) |
| `{{ #date "%Y-%m-%d %H:%M" }}` | Text | Local timestamp formatted with custom `strftime` string |
| `{{ #location }}` | Text | Current configured profile location string |

### Layout & Spacing Tokens
| Tag | Description |
|---|---|
| `{{ vspace N }}` | Adds `N` pixels of vertical blank space (default: 16) |
| `{{ hr [thickness] }}` | Draws a solid horizontal rule across the receipt (default: 1px) |

---

## Configuration

Reports are configured through `config.toml`:

```toml
[profile]
location = "Seattle, WA"
width = 512

[template]
content = '''
┌──────────────────────────────────────────────────────────────┐
{{ height 4}}│{{width 4}}{{bold}}coalescence{{\bold}}{{width 1}}                  │{{height 1}}
├──────────────────────────────────────────────────────────────┤
│Printed {{#date "%Y-%m-%d %H:%M"}}                                      │
└──────────────────────────────────────────────────────────────┘
{{#location}}

{{!weather}}

{{!astro}}
'''
```

---

## Quickstart

### Prerequisites
- **Rust Toolchain**: Rust 2024 edition (1.85+ recommended).
- **Linux USB Development Libraries** (for physical ESC/POS USB printer support):
  - Debian/Ubuntu: `sudo apt-get install libudev-dev pkg-config`
  - Fedora/RHEL: `sudo dnf install systemd-devel pkgconf-pkg-config`
  - Arch Linux: `sudo pacman -S systemd pkgconf`

### 1. Build and Run Preview
`coalescence` renders a high-fidelity preview to `output.png`:

```bash
cargo run -- config.toml
```

You can view `output.png` in any image viewer.

### 2. Printing to Hardware
To print directly to an ESC/POS USB thermal printer:
1. Locate your printer's Vendor ID (VID) and Product ID (PID) using `lsusb` (e.g., `0x0519:0x0001`).
2. Update the `UsbDriver::open(vid, pid, None, None)` call in `src/main.rs`.
3. Ensure your user has permissions to write to the USB device (via `udev` rules or group membership).
4. Uncomment `print_image(img)` in `src/main.rs`.

---

## Project Structure

```
coalescence/
├── Cargo.toml          # Package configuration & dependencies
├── config.toml         # User profile and receipt layout template
├── fonts/              # Embedded Unifont font assets
│   ├── UnifontExMono.ttf
│   └── unifont_upper-18.0.01.otf
└── src/
    ├── main.rs         # Engine, token parser, layout measurement & printing
    ├── weather.rs      # OpenMeteo client, 24H chart & 5-day matrix table
    └── astro.rs        # Ephemeris, NOAA SWPC ingestion & 24H Kp index graph
```

---

## Customizing Astro Representation

Astronomical data ingestion and visual rendering are cleanly decoupled in `src/astro.rs`:

```rust
// In src/astro.rs:
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
    pub kp_index: u8,
    pub kp_history: Vec<SwpcKpPoint>,
    pub solar_wind_spd: f32,
    pub solar_wind_den: f32,
    pub sunspot_num: u16,
    pub radio_flux: u16,
    pub flare_c: u8,
    pub flare_m: u8,
    pub flare_x: u8,
    pub geomag_status: String,
}
```

- To change the polar chart dimensions, gauge rendering, or typography, edit `generate_from_data(&AstroData)`.
- To adjust ephemeris parameters or add new space weather feeds, edit `fetch_live_data(lat, lon, tz)`.

---

## Roadmap

- [x] Dynamic canvas height (continuous roll rendering)
- [x] Embedded Unifont assets (portable binaries without path dependencies)
- [x] Live solar ephemeris via `sun`
- [x] Live space weather & Kp gauge via NOAA SWPC
- [x] 24-hour planetary Kp index chart (NOAA SWPC historical 3-hour series)
- [x] Matrix-tabulated 5-day weather outlook
- [x] Automatic coordinate and timezone resolution via OpenMeteo geocoding
- [x] Parametrized macro tags (e.g. `{{ !weather "Tokyo" }}`)
- [x] User-defined custom image macro with CSS-style margins (`{{ !image "path/to/asset.png" 10 20 }}`)
- [x] Configurable canvas width in `config.toml` (`[profile.width]`)
- [ ] Word-wrapping text blocks

---

## License

[GPLv3](LICENSE)
