# esp-light-show

`no_std` Rust firmware for the [ESPHome Starter Kit](https://esphome.io/starter-kit/)'s ESP32-C6: a
motion-triggered light show on the Casita module's 10 LEDs, logging every LED colour change to
[QuestDB](https://questdb.com) over QWP with [qwp-embedded](https://github.com/sklarsa/qwp-embedded).

- **Motion**: the current show fades in, and fades out 2 minutes after the last motion.
- **Boot button**: a short press changes the show, a long press turns everything off so motion
  can't wake it, and another long press turns it back on.
- **Shows**: drift, fire, comet, sparkle.
- **Telemetry**: `light_events` gets a row per LED colour change, up to ~300/s. `health` gets a row
  a second, under `device = 'esp-light-show'`. `motion_events` gets a row each time the PIR changes,
  even with the lights off. Rows wait in a ~50 s ring until QuestDB acks them.

The light show doesn't depend on the network: without WiFi or QuestDB it just runs.

## Hardware

The LED module and the motion module on the two ribbon ports, either way round.

| Pin | |
|---|---|
| GPIO4 | accessory power, high powers the modules |
| GPIO14 | LEDs, WS2812 (driven by SPI + DMA so WiFi interrupts can't corrupt them) |
| GPIO3 | PIR, high on motion |
| GPIO9 | boot button, low when pressed |

## Tuning

The knobs are at the top of [`src/lib.rs`](src/lib.rs): `MAX_BRIGHTNESS`, `HOLD_MS`, the fade times
and `LONG_PRESS_MS`.

## Build and flash

`flash.sh` reads WiFi credentials from a homelab sops file next to this repo. Otherwise:

```sh
WIFI_SSID=... WIFI_PASSWORD=... QUESTDB_ADDR=192.168.1.10:9000 cargo run --release
```

The logic runs on the host against a checkout of qwp-embedded:

```sh
rustc +stable --edition 2024 --crate-type rlib --crate-name qwp_embedded ../qwp-embedded/src/lib.rs -o /tmp/libqwp_embedded.rlib
rustc +stable --edition 2024 --test src/lib.rs --extern qwp_embedded=/tmp/libqwp_embedded.rlib -o /tmp/light-tests && /tmp/light-tests
```

To work on the crate alongside this firmware, add this to `Cargo.toml`, and don't commit it:

```toml
[patch."https://github.com/sklarsa/qwp-embedded"]
qwp-embedded = { path = "../qwp-embedded" }
```

## QuestDB tables

Create them first, as QWP would otherwise pick wide column types and no TTL:

```sql
CREATE TABLE IF NOT EXISTS light_events (ts TIMESTAMP, led BYTE, show SYMBOL, r SHORT, g SHORT, b SHORT)
  TIMESTAMP(ts) PARTITION BY DAY TTL 7 DAYS WAL DEDUP UPSERT KEYS(ts, led);
CREATE TABLE IF NOT EXISTS motion_events (ts TIMESTAMP, active BYTE) -- 1 started, 0 ended
  TIMESTAMP(ts) PARTITION BY MONTH TTL 3 MONTHS WAL DEDUP UPSERT KEYS(ts);
```

`health` is shared with other devices, keyed on `(ts, device)`.
