#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

//! ESPHome Starter Kit ESP32-C6: motion-triggered light show on the Casita module's 10 LEDs. The
//! boot button changes the show; a long press turns it off. Every LED colour change goes to
//! QuestDB, and a dropped connection never touches the lights.

use core::cell::{Cell, RefCell};
use core::mem::MaybeUninit;
use core::net::{IpAddr, SocketAddr};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering::Relaxed};

use embassy_executor::Spawner;
use embassy_net::{
    Runner, Stack, StackResources,
    dns::DnsQueryType,
    udp::{PacketMetadata, UdpSocket},
};
use embassy_sync::blocking_mutex::Mutex;
use embassy_time::{Duration, Instant, Ticker, Timer, with_timeout};
use esp_backtrace as _;
use esp_hal::{
    clock::CpuClock,
    dma_rx_buffer, dma_tx_buffer,
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    rng::Rng,
    spi::{
        Mode,
        master::{Config as SpiConfig, Spi},
    },
    time::Rate,
    timer::timg::TimerGroup,
};
use esp_light_show::{
    Controller, DEVICE, FRAME_MS, HEALTH_COLUMNS, LEDS, Pixels, Prng, Rec, SHOWS, SYMBOLS,
    WS_BYTES, changed, dim, encode, render, ws2812,
};
use esp_radio::wifi::{
    AuthenticationMethodConfig, Config, ControllerConfig, Interface, PowerSaveMode, WifiController,
    sta::StationConfig,
};
use log::{error, info, warn};
use qwp_embedded::Ring;
use qwp_embedded::sender::{self, Buffers, SharedRing, Stats};
use sntpc::{NtpContext, NtpTimestampGenerator, get_time};
use sntpc_net_embassy::UdpSocketWrapper;

esp_bootloader_esp_idf::esp_app_desc!();

macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write($val);
        x
    }};
}

const SSID: &str = env!("WIFI_SSID");
const PASSWORD: &str = env!("WIFI_PASSWORD");
const QUESTDB: &str = env!("QUESTDB_ADDR"); // ip:port of QuestDB's HTTP endpoint
const NTP_SERVER: &str = "pool.ntp.org";

// This board's share of the sender's memory: no PSRAM, ~512 KB of RAM to split with WiFi.
const RING_RECS: usize = 1536; // ~50 s of the busiest show
const SCRATCH_RECS: usize = 96;
const BODY_LEN: usize = 16 * 1024;
const TCP_TX_LEN: usize = 16 * 1024;

const RESET_GAP_BYTES: usize = 32; // low for 80 µs: WS2812s latch after 50 µs

static OFFSET_US: critical_section::Mutex<Cell<i64>> = critical_section::Mutex::new(Cell::new(0));
static SYNCED: AtomicBool = AtomicBool::new(false);
static QWP_STATS: Stats = Stats::new();
static RSSI_DBM: AtomicI32 = AtomicI32::new(0);
static AP_BSSID: critical_section::Mutex<Cell<u64>> = critical_section::Mutex::new(Cell::new(0));
static WIFI_CHANNEL: AtomicU32 = AtomicU32::new(0);
static WIFI_DISCONNECTS: AtomicU32 = AtomicU32::new(0);
/// Health that didn't fit in a full ring; it still has to get out, to show the ring is full.
static LATEST_HEALTH: critical_section::Mutex<Cell<Option<Rec>>> =
    critical_section::Mutex::new(Cell::new(None));

/// A panic reboots rather than leaving the lights frozen.
#[unsafe(no_mangle)]
fn custom_halt() -> ! {
    esp_hal::system::software_reset()
}

fn now_us() -> i64 {
    Instant::now().as_micros() as i64 + critical_section::with(|cs| OFFSET_US.borrow(cs).get())
}

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    esp_println::logger::init_logger_from_env();
    let reset_reason = esp_hal::system::reset_reason().map_or(0, |r| r as i32);
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

    esp_alloc::heap_allocator!(size: 96 * 1024);
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    // Powers both module ports, before anything reads or drives them.
    let accessory_power = mk_static!(
        Output<'static>,
        Output::new(peripherals.GPIO4, Level::High, OutputConfig::default())
    );
    let _ = accessory_power;
    Timer::after_millis(10).await;

    let motion = Input::new(
        peripherals.GPIO3,
        InputConfig::default().with_pull(Pull::Down), // reads "no motion" if the module's unplugged
    );
    let button = Input::new(
        peripherals.GPIO9,
        InputConfig::default().with_pull(Pull::Up),
    );

    // SPI + DMA, not RMT: the C6 has one core, and RMT's 48-symbol buffer needs refilling from an
    // interrupt in the middle of a frame, which WiFi can delay long enough to corrupt colours.
    let mut leds = Spi::new(
        peripherals.SPI2,
        SpiConfig::default()
            .with_frequency(Rate::from_khz(3200))
            .with_mode(Mode::_0),
    )
    .unwrap()
    .with_mosi(peripherals.GPIO14)
    .with_dma(peripherals.DMA_CH0)
    .with_buffers(
        dma_rx_buffer!(4).unwrap(),
        dma_tx_buffer!(WS_BYTES + RESET_GAP_BYTES).unwrap(),
    );

    // The ring is far too big to build on the stack, and needs no initialising.
    static RING_MEM: static_cell::StaticCell<[MaybeUninit<Rec>; RING_RECS]> =
        static_cell::StaticCell::new();
    // SAFETY: an array of MaybeUninit is valid uninitialised
    let slots = unsafe { RING_MEM.uninit().assume_init_mut() };
    let ring: &'static SharedRing<Rec> = mk_static!(
        SharedRing<Rec>,
        Mutex::new(RefCell::new(Ring::new(&mut slots[..])))
    );
    info!(
        "ring: {RING_RECS} x {} B = {} KB",
        size_of::<Rec>(),
        RING_RECS * size_of::<Rec>() / 1024
    );

    let station = Config::Station(
        StationConfig::default()
            .with_ssid(SSID.try_into().unwrap())
            .with_authentication(AuthenticationMethodConfig::Wpa2Personal(
                PASSWORD.try_into().unwrap(),
            )),
    );
    let mut controller = WifiController::new(
        peripherals.WIFI,
        ControllerConfig::default().with_initial_config(station),
    )
    .unwrap();
    controller.set_power_saving(PowerSaveMode::None).unwrap();
    let rng = Rng::new();
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;
    let (stack, runner) = embassy_net::new(
        Interface::station(),
        embassy_net::Config::dhcpv4(Default::default()),
        mk_static!(StackResources<4>, StackResources::<4>::new()),
        seed,
    );
    spawner.spawn(connection(controller).unwrap());
    spawner.spawn(net_task(runner).unwrap());
    spawner.spawn(sntp_task(stack).unwrap());
    let buffers = Buffers {
        body: mk_static!([u8; BODY_LEN], [0; BODY_LEN]),
        tcp_tx: mk_static!([u8; TCP_TX_LEN], [0; TCP_TX_LEN]),
        tcp_rx: mk_static!([u8; 2048], [0; 2048]),
        scratch: mk_static!(
            [MaybeUninit<Rec>; SCRATCH_RECS],
            [const { MaybeUninit::uninit() }; SCRATCH_RECS]
        ),
    };
    spawner.spawn(qwp_task(stack, ring, buffers).unwrap());

    info!("{DEVICE}: ready, show {}", SHOWS[0]);
    let mut show = Controller::new();
    let mut prng = Prng(rng.random() | 1);
    let (mut px, mut sent): (Pixels, Pixels) = ([[0; 3]; LEDS], [[0; 3]; LEDS]);
    let mut wire = [0u8; WS_BYTES + RESET_GAP_BYTES];
    let mut ticker = Ticker::every(Duration::from_millis(FRAME_MS));
    let mut next_health = Instant::now();
    let (mut was_motion, mut was_show, mut was_enabled) = (false, 0, true);
    let (mut logged_motion, mut was_synced) = (false, false);
    loop {
        let now_ms = Instant::now().as_millis();
        let is_motion = motion.is_high();
        show.tick(now_ms, is_motion, button.is_low());
        let synced = SYNCED.load(Relaxed);
        // Edges only count once the clock is right, so log the level as it stands at the first
        // sync: otherwise the first logged edge could be an end with no start.
        if synced && (is_motion != logged_motion || !was_synced) {
            record(
                ring,
                Rec::Motion {
                    t_us: now_us(),
                    active: is_motion,
                },
            );
            logged_motion = is_motion;
        }
        was_synced = synced;
        if is_motion != was_motion || show.show != was_show || show.enabled != was_enabled {
            info!(
                "motion={is_motion} show={} enabled={}",
                SHOWS[show.show], show.enabled
            );
            (was_motion, was_show, was_enabled) = (is_motion, show.show, show.enabled);
        }

        render(show.show, now_ms, &mut prng, &mut px);
        let out = dim(&px, show.level);
        let mask = changed(&sent, &out);
        if mask != 0 {
            let (colours, _) = wire.split_at_mut(WS_BYTES);
            ws2812(&out, colours.try_into().unwrap());
            if let Err(e) = leds.write(&wire) {
                error!("leds: {e:?}");
            }
            if synced {
                record(
                    ring,
                    Rec::Frame {
                        t_us: now_us(),
                        show: show.show as u8,
                        changed: mask,
                        rgb: out,
                    },
                );
            }
            sent = out;
        }

        if !synced {
            next_health = Instant::now(); // don't owe a row for every second before the sync
        } else if Instant::now() >= next_health {
            next_health += Duration::from_secs(1);
            report_health(ring, reset_reason, is_motion);
        }
        ticker.next().await;
    }
}

fn record(ring: &SharedRing<Rec>, rec: Rec) {
    ring.lock(|r| r.borrow_mut().push(rec));
}

fn report_health(ring: &SharedRing<Rec>, reset_reason: i32, motion: bool) {
    ring.lock(|r| {
        let mut r = r.borrow_mut();
        let s = &QWP_STATS;
        let values: [i32; HEALTH_COLUMNS.len()] = [
            r.dropped as i32,
            s.tx_bytes.swap(0, Relaxed) as i32,
            s.encode_us.swap(0, Relaxed) as i32,
            RSSI_DBM.load(Relaxed),
            WIFI_CHANNEL.load(Relaxed) as i32,
            esp_alloc::HEAP.free() as i32,
            WIFI_DISCONNECTS.load(Relaxed) as i32,
            s.reconnects.load(Relaxed) as i32,
            r.len() as i32,
            s.acks.load(Relaxed) as i32,
            s.rejected.load(Relaxed) as i32,
            s.last_error.load(Relaxed) as i32,
            Instant::now().as_secs() as i32,
            reset_reason,
            motion as i32,
        ];
        let health = Rec::Health {
            t_us: now_us(),
            ap_bssid: critical_section::with(|cs| AP_BSSID.borrow(cs).get()),
            values,
        };
        if r.len() == r.capacity() {
            critical_section::with(|cs| LATEST_HEALTH.borrow(cs).set(Some(health)));
        } else {
            r.push(health);
        }
    });
}

#[embassy_executor::task]
async fn qwp_task(stack: Stack<'static>, ring: &'static SharedRing<Rec>, buffers: Buffers<Rec>) {
    let rng = Rng::new();
    let config = sender::Config {
        addr: QUESTDB.parse().expect("QUESTDB_ADDR must be ipv4:port"),
        client_id: concat!("esp-light-show/", env!("CARGO_PKG_VERSION")),
        symbols: &SYMBOLS,
        batch_interval: Duration::from_millis(250),
        stats: &QWP_STATS,
    };
    sender::run(
        stack,
        ring,
        buffers,
        &config,
        || rng.random(),
        || critical_section::with(|cs| LATEST_HEALTH.borrow(cs).take()),
        encode,
    )
    .await
}

#[embassy_executor::task]
async fn connection(mut controller: WifiController<'static>) {
    loop {
        match controller.connect_async().await {
            Ok(_) => {
                info!("wifi connected");
                let gone = loop {
                    if let Ok(r) = with_timeout(
                        Duration::from_secs(1),
                        controller.wait_for_disconnect_async(),
                    )
                    .await
                    {
                        break r;
                    }
                    RSSI_DBM.store(controller.rssi().unwrap_or(0), Relaxed);
                    if let Ok(ap) = controller.ap_info() {
                        let bssid = ap.bssid.iter().fold(0u64, |a, b| a << 8 | *b as u64);
                        critical_section::with(|cs| AP_BSSID.borrow(cs).set(bssid));
                        WIFI_CHANNEL.store(ap.channel as u32, Relaxed);
                    }
                };
                RSSI_DBM.store(0, Relaxed);
                critical_section::with(|cs| AP_BSSID.borrow(cs).set(0));
                WIFI_CHANNEL.store(0, Relaxed);
                WIFI_DISCONNECTS.fetch_add(1, Relaxed);
                warn!("wifi disconnected: {gone:?}");
            }
            Err(e) => error!("wifi connect failed: {e:?}"),
        }
        Timer::after_secs(5).await
    }
}

#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, Interface>) {
    runner.run().await
}

#[derive(Clone, Copy)]
struct NtpNow(i64);

impl NtpTimestampGenerator for NtpNow {
    fn init(&mut self) {
        self.0 = now_us();
    }
    fn timestamp_sec(&self) -> u64 {
        (self.0 / 1_000_000) as u64
    }
    fn timestamp_subsec_micros(&self) -> u32 {
        (self.0 % 1_000_000) as u32
    }
}

/// Each sync steps the offset. ponytail: steps are ~ms on a LAN, slew if that ever matters.
#[embassy_executor::task]
async fn sntp_task(stack: Stack<'static>) {
    let (mut rx_meta, mut tx_meta) = ([PacketMetadata::EMPTY; 4], [PacketMetadata::EMPTY; 4]);
    let (mut rx, mut tx) = ([0u8; 512], [0u8; 512]);
    stack.wait_config_up().await;
    let mut sock = UdpSocket::new(stack, &mut rx_meta, &mut rx, &mut tx_meta, &mut tx);
    sock.bind(123).unwrap();
    let sock = UdpSocketWrapper::new(sock);
    loop {
        let synced = match stack.dns_query(NTP_SERVER, DnsQueryType::A).await {
            Ok(addrs) if !addrs.is_empty() => {
                let server = SocketAddr::from((IpAddr::from(addrs[0]), 123));
                match get_time(server, &sock, NtpContext::new(NtpNow(0))).await {
                    Ok(t) => {
                        critical_section::with(|cs| {
                            let o = OFFSET_US.borrow(cs);
                            o.set(o.get() + t.offset());
                        });
                        info!("sntp: offset step {} µs", t.offset());
                        SYNCED.store(true, Relaxed);
                        true
                    }
                    Err(e) => {
                        warn!("sntp: {e:?}");
                        false
                    }
                }
            }
            r => {
                warn!("sntp dns: {r:?}");
                false
            }
        };
        Timer::after_secs(if synced { 300 } else { 5 }).await;
    }
}
