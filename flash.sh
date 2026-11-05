#!/bin/sh
# ./flash.sh [port]   build and flash over USB. WiFi comes from homelab's secrets.
set -e
cd "$(dirname "$0")"
s() { sops -d --extract "[\"$1\"]" ../homelab/pi5/secrets.yaml; }
export WIFI_SSID="$(s wifi_ssid)" WIFI_PASSWORD="$(s wifi_password)"
export QUESTDB_ADDR="${QUESTDB_ADDR:-192.168.7.74:9000}"
cargo build --release
exec espflash flash --chip esp32c6 --non-interactive -p "${1:-/dev/ttyACM0}" target/riscv32imac-unknown-none-elf/release/esp-light-show
