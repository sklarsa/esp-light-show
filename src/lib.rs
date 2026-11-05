//! Everything that doesn't touch hardware: the shows, the button and motion logic, the LED wire
//! format and the telemetry encoding. Tested on the host (see the README).
#![cfg_attr(not(test), no_std)]

use qwp_embedded as qwp;

pub const LEDS: usize = 10;
pub type Rgb = [u8; 3];
pub type Pixels = [Rgb; LEDS];

// ---- Knobs. Tune these against the real hardware and room. ----
/// Top brightness out of 255. Ten full-white WS2812s pull ~600 mA from USB, and it's a lot of light.
pub const MAX_BRIGHTNESS: u32 = 40;
pub const FRAME_MS: u64 = 33;
/// The show stays up this long after the last motion.
pub const HOLD_MS: u64 = 120_000;
pub const FADE_IN_MS: u64 = 800;
pub const FADE_OUT_MS: u64 = 3_000;
pub const LONG_PRESS_MS: u64 = 800;

pub const DEVICE: &str = "esp-light-show";
pub const SHOWS: [&str; 4] = ["drift", "fire", "comet", "sparkle"];
/// The QWP symbol dictionary: the device, then the show names.
pub const SYMBOLS: [&str; 1 + SHOWS.len()] = [DEVICE, SHOWS[0], SHOWS[1], SHOWS[2], SHOWS[3]];

// ---- Button and motion ----

/// Short press: next show (and wakes it). Long press: off, so motion can't wake it; again for on.
pub struct Controller {
    pub enabled: bool,
    pub show: usize,
    /// Fade position, 0-255.
    pub level: u8,
    pressed_at: Option<u64>,
    long_fired: bool,
    active_until: u64,
    last_ms: u64,
}

impl Default for Controller {
    fn default() -> Self {
        Self::new()
    }
}

impl Controller {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            show: 0,
            level: 0,
            pressed_at: None,
            long_fired: false,
            active_until: 0,
            last_ms: 0,
        }
    }

    pub fn tick(&mut self, now_ms: u64, motion: bool, button: bool) {
        match (button, self.pressed_at) {
            (true, None) => {
                self.pressed_at = Some(now_ms);
                self.long_fired = false;
            }
            (true, Some(at)) if !self.long_fired && now_ms - at >= LONG_PRESS_MS => {
                self.long_fired = true;
                self.enabled = !self.enabled;
                self.active_until = if self.enabled { now_ms + HOLD_MS } else { 0 };
            }
            (false, Some(_)) => {
                self.pressed_at = None;
                if !self.long_fired && self.enabled {
                    // ponytail: a press while faded out also skips a show, cheap to live with
                    self.show = (self.show + 1) % SHOWS.len();
                    self.active_until = now_ms + HOLD_MS;
                }
            }
            _ => {}
        }
        if self.enabled && motion {
            self.active_until = now_ms + HOLD_MS;
        }

        let dt = now_ms.saturating_sub(self.last_ms);
        self.last_ms = now_ms;
        let level = self.level as u64;
        self.level = if now_ms < self.active_until {
            (level + (dt * 255).div_ceil(FADE_IN_MS)).min(255)
        } else {
            level.saturating_sub((dt * 255).div_ceil(FADE_OUT_MS))
        } as u8;
    }
}

// ---- Shows ----

pub struct Prng(pub u32);

impl Prng {
    /// xorshift32. The seed must not be 0.
    pub fn next_u32(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
}

/// A fully saturated colour; `hue` wraps at 256.
fn wheel(hue: u8) -> Rgb {
    let (sector, f) = (hue / 43, (hue % 43) * 6);
    match sector {
        0 => [255, f, 0],
        1 => [255 - f, 255, 0],
        2 => [0, 255, f],
        3 => [0, 255 - f, 255],
        4 => [f, 0, 255],
        _ => [255, 0, 255 - f],
    }
}

fn scale(c: Rgb, by: u32) -> Rgb {
    c.map(|v| (v as u32 * by / 255) as u8)
}

/// Draws one frame into `px`, which holds the previous frame for the shows that decay.
pub fn render(show: usize, t_ms: u64, rng: &mut Prng, px: &mut Pixels) {
    match SHOWS[show] {
        "drift" => {
            for (i, p) in px.iter_mut().enumerate() {
                *p = wheel(((t_ms / 40) as usize + i * 10) as u8);
            }
        }
        "fire" => {
            for p in px.iter_mut() {
                let (r, flame) = (rng.next_u32(), p[0] as u32);
                // drift towards a random heat so it flickers instead of strobing
                let heat = (flame * 3 + 60 + r % 196) / 4;
                *p = [heat as u8, (heat * heat / 900) as u8, 0];
            }
        }
        "comet" => {
            let span = 2 * (LEDS as u64 - 1);
            let step = (t_ms / 70) % span;
            let head = if step < LEDS as u64 {
                step
            } else {
                span - step
            } as usize;
            let colour = wheel((t_ms / 100) as u8);
            for p in px.iter_mut() {
                *p = scale(*p, 170);
            }
            px[head] = colour;
        }
        _ => {
            // sparkle
            for p in px.iter_mut() {
                *p = scale(*p, 225);
            }
            if rng.next_u32().is_multiple_of(4) {
                let r = rng.next_u32();
                px[r as usize % LEDS] = wheel((r >> 8) as u8);
            }
        }
    }
}

/// The values to actually send: the frame at the current fade and the brightness cap.
pub fn dim(px: &Pixels, level: u8) -> Pixels {
    px.map(|c| scale(c, level as u32 * MAX_BRIGHTNESS / 255))
}

/// Bit `i` is set if LED `i` differs.
pub fn changed(a: &Pixels, b: &Pixels) -> u16 {
    (0..LEDS).fold(0, |m, i| m | ((a[i] != b[i]) as u16) << i)
}

// ---- LED wire format ----

/// SPI bytes per frame: 24 colour bits per LED, at 4 SPI bits each.
pub const WS_BYTES: usize = LEDS * 12;

/// WS2812 over SPI at 3.2 MHz, 312.5 ns per SPI bit: a 0 is `1000`, a 1 is `1110`. Colours go out GRB.
pub fn ws2812(px: &Pixels, out: &mut [u8; WS_BYTES]) {
    let mut bytes = out.iter_mut();
    for [r, g, b] in px {
        for colour in [g, b, r] {
            for pair in [6, 4, 2, 0] {
                let code = |bit: u8| {
                    if colour >> bit & 1 == 1 {
                        0b1110
                    } else {
                        0b1000
                    }
                };
                *bytes.next().unwrap() = code(pair + 1) << 4 | code(pair);
            }
        }
    }
}

// ---- Telemetry ----

#[derive(Clone, Copy)]
pub enum Rec {
    /// One rendered frame: bit `i` of `changed` means LED `i` took the colour `rgb[i]`.
    Frame {
        t_us: i64,
        show: u8,
        changed: u16,
        rgb: Pixels,
    },
    Health {
        t_us: i64,
        /// The connected AP's MAC as an integer, 0 when not associated.
        ap_bssid: u64,
        /// In `HEALTH_COLUMNS` order. 32 bits to keep ring slots small: this variant sizes them.
        values: [i32; HEALTH_COLUMNS.len()],
    },
}

/// The columns of the shared `health` table that this board can fill, besides `ap_bssid`.
pub const HEALTH_COLUMNS: [&str; 14] = [
    "dropped",
    "tx_bytes",
    "encode_us",
    "rssi_dbm",
    "wifi_channel",
    "heap_free",
    "wifi_disconnects",
    "qwp_reconnects",
    "ring_len",
    "qwp_acks",
    "qwp_rejected",
    "qwp_last_error",
    "uptime_s",
    "reset_reason",
];

/// Writes records as up to two table blocks: `light_events` and `health`.
pub fn encode(e: &mut qwp::Encoder<'_>, recs: &[Rec]) -> Result<(), qwp::Full> {
    use qwp::{BYTE, LONG, SHORT, SYMBOL};

    let events = || {
        recs.iter()
            .filter_map(|r| match r {
                Rec::Frame {
                    t_us,
                    show,
                    changed,
                    rgb,
                } => Some((*t_us, *show, *changed, rgb)),
                _ => None,
            })
            .flat_map(|(t, show, mask, rgb)| {
                (0..LEDS)
                    .filter(move |i| mask >> i & 1 == 1)
                    .map(move |i| (t, show, i, rgb[i]))
            })
    };
    let rows = events().count();
    if rows > 0 {
        e.table(
            "light_events",
            rows,
            &[
                ("led", BYTE),
                ("show", SYMBOL),
                ("r", SHORT),
                ("g", SHORT),
                ("b", SHORT),
            ],
        )?;
        e.i8s(events().map(|(_, _, i, _)| i as i8))?;
        e.symbols(events().map(|(_, s, _, _)| 1 + s as u64))?;
        for c in 0..3 {
            e.i16s(events().map(|(_, _, _, rgb)| rgb[c] as i16))?;
        }
        e.timestamps(events().map(|(t, ..)| t))?;
    }

    let health = || {
        recs.iter().filter_map(|r| match r {
            Rec::Health {
                t_us,
                ap_bssid,
                values,
            } => Some((*t_us, *ap_bssid, values)),
            _ => None,
        })
    };
    let rows = health().count();
    if rows > 0 {
        let mut cols = [("ap_bssid", LONG); 2 + HEALTH_COLUMNS.len()];
        cols[0] = ("device", SYMBOL);
        for (c, name) in cols[2..].iter_mut().zip(HEALTH_COLUMNS) {
            *c = (name, LONG);
        }
        e.table("health", rows, &cols)?;
        e.symbols(health().map(|_| 0))?;
        e.i64s(health().map(|(_, bssid, _)| bssid as i64))?;
        for c in 0..HEALTH_COLUMNS.len() {
            e.i64s(health().map(|(_, _, v)| v[c] as i64))?;
        }
        e.timestamps(health().map(|(t, ..)| t))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_press_cycles_shows_and_wraps() {
        let mut c = Controller::new();
        for (i, want) in [1, 2, 3, 0].into_iter().enumerate() {
            let t = 1000 * i as u64;
            c.tick(t, false, true);
            c.tick(t + 100, false, false);
            assert_eq!(c.show, want);
        }
        assert!(c.enabled);
    }

    #[test]
    fn long_press_turns_off_once_and_motion_stays_ignored() {
        let mut c = Controller::new();
        c.tick(0, true, false);
        c.tick(1000, true, false);
        assert_eq!(c.level, 255);
        c.tick(2000, false, true);
        c.tick(2000 + LONG_PRESS_MS, false, true);
        c.tick(2000 + LONG_PRESS_MS * 2, false, true);
        assert!(!c.enabled, "toggled once, however long it's held");
        c.tick(4000, false, false);
        assert_eq!(c.show, 0, "a long press isn't also a short one");
        c.tick(10_000, true, false);
        assert!(!c.enabled);
        assert_eq!(c.level, 0, "faded out, and motion doesn't wake it");

        c.tick(20_000, false, true);
        c.tick(20_000 + LONG_PRESS_MS, false, true);
        c.tick(20_000 + LONG_PRESS_MS + 50, false, false);
        c.tick(21_500, false, false);
        assert!(c.enabled);
        assert!(c.level > 0, "turning it on shows it right away");
    }

    #[test]
    fn fades_out_after_the_hold() {
        let mut c = Controller::new();
        c.tick(0, true, false);
        c.tick(FADE_IN_MS, false, false);
        assert_eq!(c.level, 255);
        c.tick(HOLD_MS - 1, false, false);
        assert_eq!(c.level, 255);
        c.tick(HOLD_MS + FADE_OUT_MS, false, false);
        assert_eq!(c.level, 0);
    }

    #[test]
    fn wheel_stays_in_range_and_hits_primaries() {
        assert_eq!(wheel(0), [255, 0, 0]);
        for h in 0..=255 {
            let c = wheel(h);
            assert!(c.contains(&0) || c.contains(&255), "{h}: {c:?}");
        }
    }

    #[test]
    fn every_show_lights_something_and_moves() {
        for show in 0..SHOWS.len() {
            let (mut rng, mut px) = (Prng(1), [[0; 3]; LEDS]);
            let mut moved = false;
            let mut lit = false;
            for f in 0..200 {
                let before = px;
                render(show, f * FRAME_MS, &mut rng, &mut px);
                moved |= before != px;
                lit |= px.iter().any(|c| c.iter().any(|v| *v > 0));
            }
            assert!(lit && moved, "{}", SHOWS[show]);
        }
    }

    #[test]
    fn dim_caps_brightness() {
        let px = [[255; 3]; LEDS];
        assert_eq!(dim(&px, 255)[0][0] as u32, MAX_BRIGHTNESS);
        assert_eq!(dim(&px, 0)[0], [0; 3]);
    }

    #[test]
    fn changed_marks_only_differing_leds() {
        let (a, mut b) = ([[1; 3]; LEDS], [[1; 3]; LEDS]);
        assert_eq!(changed(&a, &b), 0);
        b[0][2] = 9;
        b[9][0] = 9;
        assert_eq!(changed(&a, &b), 0b10_0000_0001);
    }

    /// First LED green 0x80 and blue 0x01, red 0: MSB of green first.
    #[test]
    fn ws2812_is_grb_msb_first() {
        let mut px = [[0; 3]; LEDS];
        px[0] = [0, 0x80, 0x01];
        let mut out = [0; WS_BYTES];
        ws2812(&px, &mut out);
        assert_eq!(&out[..4], &[0xE8, 0x88, 0x88, 0x88]); // g
        assert_eq!(&out[4..8], &[0x88, 0x88, 0x88, 0x8E]); // b
        assert_eq!(&out[8..12], &[0x88; 4]); // r
        assert!(out[12..].iter().all(|b| *b == 0x88));
    }

    #[test]
    fn encodes_one_row_per_changed_led_with_real_values() {
        let mut rgb = [[0; 3]; LEDS];
        rgb[3] = [200, 7, 9];
        let rec = Rec::Frame {
            t_us: 1_000_000,
            show: 2,
            changed: 0b1001,
            rgb,
        };
        let mut buf = [0u8; 256];
        let mut e = qwp::Encoder::with_symbols(&mut buf, 0, &SYMBOLS).unwrap();
        encode(&mut e, &[rec]).unwrap();
        let n = e.finish();
        let b = &buf[..n];
        let find = |w: &[u8]| b.windows(w.len()).position(|x| x == w).unwrap();
        // led column: null flag, then LEDs 0 and 3
        let name = find(b"light_events");
        assert_eq!(b[name + 12], 2, "two rows");
        let s = find(&[0, 0, 3, 0, 3, 3]); // led 0,3 then null flag + symbol id 3 ("comet") twice
        assert!(s > name);
        assert_eq!(SYMBOLS[3], "comet");
        assert!(
            b.windows(4).any(|w| w == [0, 0, 200, 0]),
            "r column keeps 200"
        );
    }

    #[test]
    fn health_block_names_the_device() {
        let rec = Rec::Health {
            t_us: 1_000_000,
            ap_bssid: 0xfedc_ba98_7654,
            values: [-57; HEALTH_COLUMNS.len()],
        };
        let mut buf = [0u8; 1024];
        let mut e = qwp::Encoder::new(&mut buf).unwrap();
        encode(&mut e, &[rec]).unwrap();
        let n = e.finish();
        assert!(buf[..n].windows(6).any(|w| w == b"health"));
        assert!(buf[..n].windows(6).any(|w| w == b"device"));
        assert!(buf[..n].windows(8).any(|w| w == (-57i64).to_le_bytes()));
        assert!(
            buf[..n]
                .windows(8)
                .any(|w| w == 0xfedc_ba98_7654i64.to_le_bytes())
        );
    }
}
