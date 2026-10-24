#!/usr/bin/env bash
# Downloads the FreePats sound packs into src-tauri/resources/packs (they are
# too big for git). Needs curl, tar and 7z (7-Zip). See packs/CREDITS.md.
set -euo pipefail
cd "$(dirname "$0")/../src-tauri/resources/packs"
F=https://freepats.zenvoid.org
G=https://github.com/freepats
SEVENZ=$(command -v 7z || echo "/c/Program Files/7-Zip/7z.exe")
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

get() { # <url> <pack name>
  [ -f "$2.sf2" ] && { echo "have $2"; return; }
  echo "getting $2"
  local a="$tmp/$(basename "$1")" x="$tmp/x-$RANDOM"
  curl -sfL -o "$a" "$1"
  mkdir -p "$x"
  case "$a" in *.7z) "$SEVENZ" x -y -o"$x" "$a" >/dev/null ;; *) tar -xf "$a" -C "$x" ;; esac
  local sf; sf=$(find "$x" -iname '*.sf2' | head -1)
  cp "$sf" "$2.sf2"
  mkdir -p "licenses/$2"
  find "$(dirname "$sf")" -maxdepth 1 -iname '*.txt' -exec cp {} "licenses/$2/" \;
}

get "$F/Guitar/SpanishClassicalGuitar/SpanishClassicalGuitar-SF2-20190618.7z" "Spanish Guitar"
get "$F/Guitar/FSS-SteelStringGuitar/FSS-SteelStringGuitar-SF2-20200521.tar.xz" "Steel String Guitar"
get "$G/electric-guitar-FSBS-clean/releases/download/2026-08-07/EGuitarFSBS-clean-bridge-small-SF2-20260807.7z" "Electric Guitar Clean"
get "$F/Piano/YDP-GrandPiano/YDP-GrandPiano-SF2-20160804.tar.bz2" "Grand Piano YDP"
get "$G/fm-piano1/releases/download/2019-09-16/FM-Piano1-SF2-20190916.7z" "FM Electric Piano"
get "$G/synth-scifi/releases/download/2020-05-17/SynthSciFi-SF2-20200517.7z" "SciFi FX"
get "$G/synth-goblins/releases/download/2020-06-12/SynthGoblins-SF2-20200612.7z" "Goblins FX"
get "$G/synth-soundtrack/releases/download/20200521/SynthSoundtrack-SF2-20200521.7z" "Soundtrack FX"
get "$G/synth-crystal/releases/download/2019-08-12/SynthCrystal-SF2-20190812.7z" "Crystal FX"
echo done
