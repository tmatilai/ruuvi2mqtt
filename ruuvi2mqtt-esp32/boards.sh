#!/usr/bin/env bash
# Finds the attached ESP32 boards and their serial ports for the Makefile,
# and registers boards. It never prompts.
#
# A board on the chip's own USB port has its MAC as the USB serial number.
# For a board behind a USB-UART bridge, `register` records the bridge's
# serial number. Otherwise only `espflash board-info` tells which board is
# on a port, and it resets the board.
#
# Usage:
#   boards.sh list             a table of the attached boards
#   boards.sh detect           "<device> <port>" of the only attached board
#   boards.sh port DEVICE      the port of a registered board
#   boards.sh only-port        the port of the only attached board
#   boards.sh register DEVICE  add the only attached board to devices.conf
#   boards.sh log SECS         the serial output of ESPFLASH_PORT for SECS
#
# ESPFLASH_PORT limits detect, only-port and register to that port. WAIT
# is how long to wait for a port, in seconds (default 75). BOARDS_CONF is
# the registry (default devices/devices.conf), for callers in other
# directories.
set -euo pipefail
# Also exit on errors inside $(...). Bash 4.4 and later.
shopt -s inherit_errexit 2>/dev/null || true

CONF=${BOARDS_CONF:-devices/devices.conf}
WAIT=${WAIT:-75}

die() {
  printf '%s\n' "$*" >&2
  exit 1
}

# One "<port> <native|uart> <usb-serial>" line per ESP32 serial port.
ports() {
  local dev props vid serial
  case $(uname -s) in
  Darwin)
    # A port's USB device and interface properties come before the port.
    ioreg -r -c IOUSBHostDevice -l -w0 | awk '
      /"idVendor" = / { vid = $NF }
      /"USB Serial Number" = / { serial = $NF; gsub(/"/, "", serial) }
      /"IOCalloutDevice" = / {
        port = $NF; gsub(/"/, "", port)
        printf "%s %04x %s\n", port, vid, (serial == "" ? "-" : serial)
      }'
    ;;
  Linux)
    for dev in /dev/ttyACM* /dev/ttyUSB*; do
      [[ -e $dev ]] || continue
      props=$(udevadm info -q property -n "$dev" 2>/dev/null) || continue
      vid=$(sed -n 's/^ID_VENDOR_ID=//p' <<<"$props")
      serial=$(sed -n 's/^ID_SERIAL_SHORT=//p' <<<"$props")
      printf '%s %s %s\n' "$dev" "$vid" "${serial:--}"
    done
    ;;
  *) die "boards.sh supports macOS and Linux" ;;
  esac | sort -u | awk '
    $2 == "303a" { print $1, "native", $3 }
    # CP210x, CH34x and FTDI bridges
    $2 == "10c4" || $2 == "1a86" || $2 == "0403" { print $1, "uart", $3 }'
}

# devices.conf without comments: "<mac> <device> [<usb-serial>]".
conf() {
  [[ -f $CONF ]] || return 0
  awk '!/^[[:space:]]*#/ && NF >= 2 { print tolower($1), $2, $3 }' "$CONF"
}

# "<mac> <device>" of a port line "<port> <kind> <usb-serial>", "-" when
# unknown. With "probe", asks a bridge's board, which resets it.
identify() {
  local port=$1 kind=$2 serial=$3 probe=${4:-} mac
  if [[ $kind == native ]]; then
    mac=$(tr '[:upper:]' '[:lower:]' <<<"$serial")
  else
    mac=$(conf | awk -v s="$serial" 's != "-" && $3 == s { print $1; exit }')
    if [[ -z $mac && -n $probe ]]; then
      mac=$(espflash board-info --port "$port" --non-interactive 2>&1 |
        sed -n 's/.*MAC address: *//p' | tr '[:upper:]' '[:lower:]')
    fi
  fi
  echo "${mac:--} $(conf | awk -v m="${mac:--}" '$1 == m { d = $2; exit } END { print (d == "" ? "-" : d) }')"
}

# A table of port lines. With "probe", resets the unknown bridge boards.
table() {
  local port kind serial mac device
  printf '%-24s %-6s %-17s %s\n' PORT USB MAC DEVICE
  while read -r port kind serial; do
    read -r mac device < <(identify "$port" "$kind" "$serial" "${1:-}")
    printf '%-24s %-6s %-17s %s\n' "$port" "$kind" "$mac" "$device"
  done
}

# The port lines, limited to ESPFLASH_PORT when it is set. Waits for one.
attached() {
  local found deadline=$((SECONDS + WAIT)) waiting=''
  while :; do
    found=$(ports | awk -v p="${ESPFLASH_PORT:-}" 'p == "" || $1 == p')
    if [[ -n $found ]]; then
      printf '%s\n' "$found"
      return
    fi
    ((SECONDS < deadline)) || die "No ESP32 board found${ESPFLASH_PORT:+ on $ESPFLASH_PORT}."
    if [[ -z $waiting ]]; then
      echo "Waiting up to ${WAIT}s for a board. A board in deep sleep shows up when it wakes." >&2
      waiting=1
    fi
    sleep 1
  done
}

# The only attached board's port line. With several, the table of the
# ones known without a reset, and the hint.
only() {
  local found
  found=$(attached)
  if (($(wc -l <<<"$found") > 1)); then
    table <<<"$found" >&2
    die "Several boards are attached. $1"
  fi
  printf '%s\n' "$found"
}

cmd_list() {
  ports | table probe
}

cmd_detect() {
  local port kind serial mac device
  read -r port kind serial < <(only "Choose one with DEVICE=<name> or ESPFLASH_PORT=<port>.")
  read -r mac device < <(identify "$port" "$kind" "$serial" probe)
  [[ $mac != - ]] || die "Couldn't read the MAC of the board on $port."
  [[ $device != - ]] || die "The board on $port ($mac) isn't registered. Register it with 'make register DEVICE=<name>', or set DEVICE= to skip."
  echo "$device $port"
}

cmd_port() {
  local device=${1:?DEVICE missing} mac usb found port kind serial
  local deadline=$((SECONDS + WAIT)) probed='' waiting=''
  read -r mac usb < <(conf | awk -v d="$device" '$2 == d { print $1, $3; exit }')
  [[ -n ${mac:-} ]] || die "DEVICE '$device' isn't in $CONF."
  while :; do
    found=$(ports)
    port=$(awk -v m="$mac" -v s="${usb:--}" '
      ($2 == "native" && tolower($3) == m) || ($2 == "uart" && s != "-" && $3 == s) { print $1; exit }
    ' <<<"$found")
    if [[ -n $port ]]; then
      echo "$port"
      return
    fi
    # Without a recorded serial number, ask the unknown bridge boards. Their
    # ports stay up in deep sleep, so once is enough.
    if [[ -z $probed && -z ${usb:-} ]]; then
      while read -r port kind serial; do
        [[ $kind == uart && $(identify "$port" "$kind" "$serial") == "- -" ]] || continue
        if [[ $(identify "$port" "$kind" "$serial" probe) == "$mac "* ]]; then
          echo "$port"
          return
        fi
      done <<<"$found"
      probed=1
    fi
    ((SECONDS < deadline)) || die "$device ($mac) isn't attached."
    if [[ -z $waiting ]]; then
      echo "Waiting up to ${WAIT}s for $device. A board in deep sleep shows up when it wakes." >&2
      waiting=1
    fi
    sleep 1
  done
}

cmd_only_port() {
  local port _
  read -r port _ < <(only "Choose one with ESPFLASH_PORT=<port>.")
  echo "$port"
}

cmd_register() {
  local device=${1:?DEVICE missing} port kind serial info mac chip
  read -r port kind serial < <(only "Choose one with ESPFLASH_PORT=<port>.")
  echo "Reading board info from $port..."
  info=$(espflash board-info --port "$port" --non-interactive 2>&1) || true
  mac=$(sed -n 's/.*MAC address: *//p' <<<"$info" | tr '[:upper:]' '[:lower:]')
  [[ -n $mac ]] || die "Couldn't read the MAC address of the board on $port."
  chip=$(sed -n 's/.*Chip type: *\([^ ]*\).*/\1/p' <<<"$info")
  [[ $kind == uart ]] || serial=-

  if [[ ! -f $CONF ]]; then
    printf '# MAC-to-device-name mapping for auto-detection.\n# Format: MAC_ADDRESS  device-name  [USB-UART bridge serial]\n\n' >"$CONF"
  fi
  if conf | awk -v m="$mac" '$1 == m { found = 1 } END { exit !found }'; then
    if [[ $serial != - ]] && ! conf | awk -v m="$mac" '$1 == m && $3 != "" { found = 1 } END { exit !found }'; then
      # Write through a symlinked devices.conf.
      awk -v m="$mac" -v s="$serial" '!/^[[:space:]]*#/ && tolower($1) == m && NF == 2 { $0 = $0 "  " s } 1' "$CONF" >"$CONF.tmp"
      cat "$CONF.tmp" >"$CONF" && rm "$CONF.tmp"
      echo "Added the bridge serial number $serial to $mac in $CONF"
    else
      echo "MAC $mac is already registered in $CONF"
    fi
  else
    if [[ $serial == - ]]; then
      printf '%s  %s\n' "$mac" "$device" >>"$CONF"
    else
      printf '%s  %s  %s\n' "$mac" "$device" "$serial" >>"$CONF"
    fi
    echo "Registered $mac as $device in $CONF"
  fi

  if [[ ! -f devices/$device.mk ]]; then
    if [[ -n $chip && $chip != esp32c6 ]]; then
      sed "s/^# CHIP = .*/CHIP = $chip/" devices/device.mk.example >"devices/$device.mk"
    else
      cp devices/device.mk.example "devices/$device.mk"
    fi
    echo "Created devices/$device.mk from template (chip: ${chip:-unknown}) — edit it to set IP, hostname, etc."
  else
    echo "devices/$device.mk already exists"
  fi
}

cmd_log() {
  local secs=${1:?SECS missing} pid
  : "${ESPFLASH_PORT:?ESPFLASH_PORT missing}"
  espflash monitor --non-interactive &
  pid=$!
  sleep "$secs"
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
}

cmd=${1:-}
shift || true
case $cmd in
list) cmd_list ;;
detect) cmd_detect ;;
port) cmd_port "$@" ;;
only-port) cmd_only_port ;;
register) cmd_register "$@" ;;
log) cmd_log "$@" ;;
*) die "Usage: boards.sh list|detect|port DEVICE|only-port|register DEVICE|log SECS" ;;
esac
