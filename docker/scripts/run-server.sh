#!/usr/bin/env bash

set -Eeuo pipefail

readonly persistent_home=/nwn/home
readonly runtime_home=/nwn/run

mkdir -p "$persistent_home" "$runtime_home"

echo "[*] Linking persistent server data"
for path in database hak modules nwsync override portraits saves servervault tlk development; do
  mkdir -p "$persistent_home/$path"
  if [[ ! -e "$runtime_home/$path" ]]; then
    ln -s "$persistent_home/$path" "$runtime_home/$path"
  fi
done

for path in dialog.tlk dialogf.tlk; do
  if [[ -e "$persistent_home/$path" && ! -e "$runtime_home/$path" && ! -L "$runtime_home/$path" ]]; then
    ln -s "$persistent_home/$path" "$runtime_home/$path"
  fi
done

if [[ -e "$persistent_home/settings.tml" && ! -e "$runtime_home/settings.tml" ]]; then
  echo "[*] Linking settings.tml"
  ln -s "$persistent_home/settings.tml" "$runtime_home/settings.tml"
fi

echo "[*] Importing configuration"
if [[ -f "$persistent_home/nwn.ini" ]]; then
  echo "[*] .. nwn.ini"
  awk -f /nwn/prep-nwn-ini.awk "$persistent_home/nwn.ini" > "$runtime_home/nwn.ini"
fi

if [[ -f "$persistent_home/nwnplayer.ini" ]]; then
  echo "[*] .. nwnplayer.ini"
  cp -p "$persistent_home/nwnplayer.ini" "$runtime_home/nwnplayer.ini"
fi

if [[ -f "$persistent_home/cryptographic_secret" ]]; then
  echo "[*] .. cryptographic_secret"
  cp -a "$persistent_home/cryptographic_secret" "$runtime_home/"
fi

persist_runtime_file() {
  local name=$1
  local source_file="$runtime_home/$name"
  local destination_file="$persistent_home/$name"
  local temporary_file

  [[ -f "$source_file" && ! -L "$source_file" ]] || return 0
  temporary_file=$(mktemp "$persistent_home/.$name.XXXXXX")
  cp -p "$source_file" "$temporary_file"
  mv -f "$temporary_file" "$destination_file"
}

backup_runtime_configuration() {
  if [[ -f "$runtime_home/cryptographic_secret" && ! -L "$runtime_home/cryptographic_secret" ]]; then
    echo "[*] Backing up cryptographic_secret"
    persist_runtime_file cryptographic_secret
  fi

  if [[ -f "$runtime_home/settings.tml" && ! -L "$runtime_home/settings.tml" ]]; then
    echo "[*] Backing up settings.tml"
    persist_runtime_file settings.tml
  fi
}

tail_pid=
backup_pid=
server_pid=

cleanup() {
  status=$?
  trap - EXIT

  for pid in "$tail_pid" "$backup_pid"; do
    if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
      kill "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
    fi
  done

  backup_runtime_configuration || true

  shopt -s nullglob
  crash_logs=("$runtime_home"/nwserver-crash*.log)
  if (( ${#crash_logs[@]} > 0 )); then
    echo "[*] Server exited with status $status; preserving crash data"
    cp -a "${crash_logs[@]}" "$persistent_home/" || true
  fi

  exit "$status"
}
trap cleanup EXIT

forward_signal() {
  local signal=$1
  local signal_status=$2
  if [[ -n "$server_pid" ]] && kill -0 "$server_pid" 2>/dev/null; then
    kill -s "$signal" "$server_pid"
  else
    exit "$signal_status"
  fi
}
trap 'forward_signal TERM 143' TERM
trap 'forward_signal INT 130' INT
trap 'forward_signal HUP 129' HUP

if [[ "${NWN_TAIL_LOGS:-y}" == y ]]; then
  echo "[*] Server logs mirrored to stdout"
  tail -q -F \
    "$runtime_home/logs.0/nwserverLog1.txt" \
    "$runtime_home/logs.0/nwserverError1.txt" &
  tail_pid=$!
fi

(
  sleep 10
  backup_runtime_configuration
) &
backup_pid=$!

extra_args=()
if [[ -n "${NWN_EXTRA_ARGS:-}" ]]; then
  read -r -a extra_args <<< "${NWN_EXTRA_ARGS}"
fi

append_secret_arg() {
  local option=$1
  local file_variable=$2
  local secret_file=${!file_variable-}
  local secret

  [[ -n "$secret_file" ]] || return 0
  [[ -f "$secret_file" && -r "$secret_file" ]] \
    || { echo "[!] $file_variable must name a readable regular file" >&2; exit 1; }
  secret=$(< "$secret_file")
  server_args+=( "$option" "$secret" )
}

server_args=(
  "${extra_args[@]}"
  -port "${NWN_PORT:-5121}"
  -interactive
  -servername "${NWN_SERVERNAME:-nwnrs server}"
  -module "${NWN_MODULE:-nwnrs}"
  -publicserver "${NWN_PUBLICSERVER:-0}"
  -maxclients "${NWN_MAXCLIENTS:-96}"
  -minlevel "${NWN_MINLEVEL:-1}"
  -maxlevel "${NWN_MAXLEVEL:-40}"
  -pauseandplay "${NWN_PAUSEANDPLAY:-1}"
  -pvp "${NWN_PVP:-2}"
  -servervault "${NWN_SERVERVAULT:-1}"
  -elc "${NWN_ELC:-1}"
  -ilr "${NWN_ILR:-1}"
  -gametype "${NWN_GAMETYPE:-0}"
  -oneparty "${NWN_ONEPARTY:-0}"
  -difficulty "${NWN_DIFFICULTY:-3}"
  -autosaveinterval "${NWN_AUTOSAVEINTERVAL:-0}"
  -reloadwhenempty "${NWN_RELOADWHENEMPTY:-0}"
)

append_secret_arg -playerpassword NWN_PLAYERPASSWORD_FILE
append_secret_arg -dmpassword NWN_DMPASSWORD_FILE
append_secret_arg -adminpassword NWN_ADMINPASSWORD_FILE

if [[ -n "${NWN_NWSYNCURL:-}" ]]; then
  server_args+=( -nwsyncurl "$NWN_NWSYNCURL" )
fi
if [[ -n "${NWN_NWSYNCHASH:-}" ]]; then
  server_args+=( -nwsynchash "$NWN_NWSYNCHASH" )
fi
server_args+=( "$@" )

echo "[*] Starting nwserver on UDP port ${NWN_PORT:-5121}"
export LD_LIBRARY_PATH="${NWN_LD_LIBRARY_PATH:-}"
export LD_PRELOAD="${NWN_LD_PRELOAD:-}"

./nwserver "${server_args[@]}" &
server_pid=$!

set +e
wait "$server_pid"
status=$?
while kill -0 "$server_pid" 2>/dev/null; do
  wait "$server_pid"
  status=$?
done
set -e

exit "$status"
