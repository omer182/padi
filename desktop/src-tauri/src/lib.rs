mod device;
mod engine;
mod hub;
mod mic;
mod midi;
mod sounds;
mod studio;

use device::DeviceLink;
use engine::{Cmd, Engine, PresetInfo, SoundParams};
use hub::{Hub, Sound, Status};
use studio::{PadSample, Studio, StudioView};
use std::sync::Arc;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::path::BaseDirectory;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, State, WindowEvent, Wry};

const SOUNDFONT: &str = "resources/GeneralUser-GS.sf2";

struct AppState {
    hub: Arc<Hub>,
    engine: Option<Arc<Engine>>,
    device: Arc<DeviceLink>,
    release_item: CheckMenuItem<Wry>,
}

#[tauri::command]
fn get_status(state: State<AppState>) -> Status {
    state.hub.status()
}

#[tauri::command]
fn get_log(state: State<AppState>) -> Vec<String> {
    state.hub.log_lines()
}

#[derive(serde::Serialize)]
struct Library {
    fonts: Vec<String>,
    presets: Vec<PresetInfo>,
}

#[tauri::command]
fn get_presets(state: State<AppState>) -> Library {
    match &state.engine {
        Some(e) => Library { fonts: e.font_names(), presets: e.presets() },
        None => Library { fonts: vec![], presets: vec![] },
    }
}

#[tauri::command]
fn set_sound(state: State<AppState>, font: usize, bank: i32, program: i32) {
    let Some(e) = &state.engine else { return };
    // Drum kits live on the GM drum channel, which the pad only plays in its
    // Drums mode. So picking a kit puts the pad in Drums mode, and picking a
    // normal sound takes it back out (to Scale mode).
    // Same for leaving Sampler mode when you pick a SoundFont sound.
    let kit = bank == 128;
    midi::set_sound(&state.hub, e, if kit { 9 } else { 0 }, font, bank, program);
    if let Some(studio) = studio::get() {
        studio.activate(false);
    }
    let style = state.hub.status().device.get("st").copied();
    let want = if kit { DRUMS_STYLE } else if matches!(style, Some(DRUMS_STYLE | studio::SAMPLER_STYLE)) { 0 } else { -1 };
    if want >= 0 && style != Some(want) && !state.device.send(&format!("st {want}")) && kit {
        state.hub.log("Pad not linked: switch it to Drums mode (menu > Scales > pad 9) to play the kit");
    }
}

/// Firmware PlayStyle value for Drums mode.
const DRUMS_STYLE: i32 = 5;

// ---- Sampler / looper (see studio.rs) ----

fn studio() -> Result<&'static Arc<Studio>, String> {
    studio::get().ok_or_else(|| "Sound engine isn't running".to_string())
}

#[tauri::command]
fn get_studio() -> Result<StudioView, String> {
    Ok(studio()?.view())
}

#[tauri::command]
fn studio_activate() -> Result<(), String> {
    studio()?.activate(true);
    Ok(())
}

#[tauri::command]
fn studio_arm(on: bool) -> Result<(), String> {
    studio()?.arm(on);
    Ok(())
}

/// On-screen pad press/release (plays the kit, or records when armed).
#[tauri::command]
fn studio_pad(pad: u8, down: bool) -> Result<(), String> {
    studio()?.pad_input(pad, down, 110, studio::Source::Screen);
    Ok(())
}

#[tauri::command]
fn studio_update_pad(pad: u8, sample: PadSample) -> Result<(), String> {
    studio()?.update_pad(pad, sample);
    Ok(())
}

#[tauri::command]
fn studio_clear_pad(pad: u8) -> Result<(), String> {
    studio()?.clear_pad(pad);
    Ok(())
}

#[tauri::command]
fn studio_spread(pad: u8) -> Result<(), String> {
    studio()?.spread(pad);
    Ok(())
}

#[tauri::command]
fn kit_new(name: String) -> Result<(), String> {
    studio()?.new_kit(&name)
}

#[tauri::command]
fn kit_load(id: String) -> Result<(), String> {
    studio()?.load_kit(&id)
}

#[tauri::command]
fn kit_rename(id: String, name: String) -> Result<(), String> {
    studio()?.rename_kit(&id, &name);
    Ok(())
}

#[tauri::command]
fn kit_duplicate(id: String) -> Result<(), String> {
    studio()?.duplicate_kit(&id)
}

#[tauri::command]
fn kit_delete(id: String) -> Result<(), String> {
    studio()?.delete_kit(&id)
}

#[tauri::command]
fn studio_undo(redo: bool) -> Result<(), String> {
    studio()?.undo(redo);
    Ok(())
}

#[tauri::command]
fn studio_copy_pad(from: u8, to: u8) -> Result<(), String> {
    studio()?.copy_pad(from, to);
    Ok(())
}

#[tauri::command]
fn studio_record_into(pad: u8) -> Result<(), String> {
    studio()?.record_into(pad);
    Ok(())
}

// ---- Sound editor + My sounds ----

/// Which sound the editor is changing: the instrument or the drum kit.
fn target_channel(target: &str) -> u8 {
    if target == "drumKit" { 9 } else { 0 }
}

#[tauri::command]
fn set_sound_params(state: State<AppState>, target: String, params: SoundParams) {
    let Some(e) = &state.engine else { return };
    let ch = target_channel(&target);
    e.send(Cmd::Params(ch, params));
    state.hub.update(|s| {
        let sound = if ch == 9 { &mut s.drum_kit } else { &mut s.sound };
        if let Some(sound) = sound {
            sound.params = params;
        }
    });
}

/// Save the sound being edited into My sounds. `id` overwrites one of yours.
#[tauri::command]
fn save_my_sound(state: State<AppState>, target: String, name: String, id: Option<String>) -> Result<(), String> {
    let status = state.hub.status();
    let sound = if target_channel(&target) == 9 { status.drum_kit } else { status.sound };
    let sound = sound.ok_or("No sound selected")?;
    let id = studio()?.save_sound(id, &name, &sound);
    state.hub.update(|s| {
        let cur = if target_channel(&target) == 9 { &mut s.drum_kit } else { &mut s.sound };
        if let Some(cur) = cur {
            cur.my_sound = Some(id);
            cur.name = name.trim().to_string();
        }
    });
    Ok(())
}

#[tauri::command]
fn load_my_sound(state: State<AppState>, id: String) -> Result<(), String> {
    let e = state.engine.as_ref().ok_or("Sound engine isn't running")?;
    let mine = studio()?.find_sound(&id).ok_or("That sound is gone")?;
    let font = e.font_names().iter().position(|n| *n == mine.font_name).ok_or_else(|| {
        format!("\"{}\" needs the {} sound pack, which isn't installed", mine.name, mine.font_name)
    })?;
    let kit = mine.bank == 128;
    let sound = Sound { font, bank: mine.bank, program: mine.program, name: mine.name, params: mine.params, my_sound: Some(id) };
    midi::apply_sound(&state.hub, e, if kit { 9 } else { 0 }, sound);
    studio()?.activate(false);
    let style = state.hub.status().device.get("st").copied();
    let want = if kit { DRUMS_STYLE } else if matches!(style, Some(DRUMS_STYLE | studio::SAMPLER_STYLE)) { 0 } else { -1 };
    if want >= 0 && style != Some(want) {
        state.device.send(&format!("st {want}"));
    }
    Ok(())
}

#[tauri::command]
fn rename_my_sound(id: String, name: String) -> Result<(), String> {
    studio()?.rename_sound(&id, &name);
    Ok(())
}

#[tauri::command]
fn delete_my_sound(id: String) -> Result<(), String> {
    studio()?.delete_sound(&id);
    Ok(())
}

/// Audition a sound without changing what the pads play (spare channel 14).
#[tauri::command]
fn preview_sound(state: State<AppState>, font: usize, bank: i32, program: i32) {
    let Some(e) = state.engine.clone() else { return };
    const CH: u8 = 13;
    let notes: &[u8] = if bank == 128 { &[36, 38, 42, 38] } else { &[60, 64, 67] };
    e.send(Cmd::Program { ch: CH, font, bank, program });
    std::thread::spawn(move || {
        for &n in notes {
            e.send(Cmd::Midi(0x90 | CH, n, 100));
            std::thread::sleep(std::time::Duration::from_millis(if bank == 128 { 180 } else { 120 }));
        }
        std::thread::sleep(std::time::Duration::from_millis(600));
        for &n in notes {
            e.send(Cmd::Midi(0x80 | CH, n, 0));
        }
    });
}

#[tauri::command]
fn looper_record() -> Result<(), String> {
    studio()?.looper_record();
    Ok(())
}

/// "stop" (toggles play), "undo", "redo", "clear", "remove" (with `layer`).
#[tauri::command]
fn looper_cmd(cmd: String, layer: Option<usize>) -> Result<(), String> {
    studio()?.looper_cmd(&cmd, layer);
    Ok(())
}

#[tauri::command]
fn looper_solo(layer: usize) -> Result<(), String> {
    studio()?.looper_solo(layer);
    Ok(())
}

#[tauri::command]
fn looper_mute(layer: usize, muted: bool) -> Result<(), String> {
    studio()?.looper_mute(layer, muted);
    Ok(())
}

#[tauri::command]
fn looper_tempo(bpm: f32, bars: u32, metronome: bool) -> Result<(), String> {
    studio()?.set_tempo(bpm, bars, metronome);
    Ok(())
}

#[tauri::command]
fn export_loop() -> Result<String, String> {
    studio()?.export_loop()
}

/// Start/stop the session recorder; returns the saved file when stopping.
#[tauri::command]
fn toggle_session() -> Result<Option<String>, String> {
    studio()?.toggle_session()
}

/// Play a note from the UI (clicking a pad on screen).
#[tauri::command]
fn play_note(state: State<AppState>, ch: u8, note: u8, velocity: u8, on: bool) {
    if let Some(e) = &state.engine {
        let ch = ch & 0x0F;
        if on {
            e.send(Cmd::Midi(0x90 | ch, note & 0x7F, velocity.clamp(1, 127)));
        } else {
            e.send(Cmd::Midi(0x80 | ch, note & 0x7F, 0));
        }
    }
}

#[tauri::command]
fn panic(state: State<AppState>) {
    if let Some(e) = &state.engine {
        e.send(Cmd::Panic);
    }
}

#[tauri::command]
fn set_volume(state: State<AppState>, volume: f32) {
    if let Some(e) = &state.engine {
        e.send(Cmd::Volume(volume));
    }
    state.hub.update(|s| s.volume = volume);
}

#[tauri::command]
fn set_serial_paused(state: State<AppState>, paused: bool) {
    state.device.set_paused(&state.hub, paused);
    let _ = state.release_item.set_checked(paused);
}

/// Load every .sf2 in these folders as an extra font, in the background (big
/// banks take a few seconds), then tell the UI the sound list changed.
fn load_packs(hub: Arc<Hub>, engine: Arc<Engine>, dirs: Vec<std::path::PathBuf>) {
    std::thread::spawn(move || {
        let mut files: Vec<_> = dirs
            .iter()
            .filter_map(|d| std::fs::read_dir(d).ok())
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("sf2")))
            .collect();
        files.sort();
        for path in files {
            let name = path.file_stem().map(|s| s.to_string_lossy().replace(['_', '-'], " ")).unwrap_or_default();
            match engine.add_font(&path, &name) {
                Ok(_) => hub.log(format!("Sounds loaded: {name}")),
                Err(e) => hub.log(format!("Couldn't load {name}: {e}")),
            }
            hub.emit("library", ());
        }
    });
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<CheckMenuItem<Wry>> {
    let show = MenuItem::with_id(app, "show", "Show PADI", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "panic", "Stop all sound", true, None::<&str>)?;
    let release =
        CheckMenuItem::with_id(app, "release", "Release pad port (for flashing)", true, false, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&show, &stop, &release, &sep, &quit])?;

    TrayIconBuilder::with_id("padi")
        .icon(app.default_window_icon().cloned().expect("app icon"))
        .tooltip("PADI")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, ev| {
            let state = app.state::<AppState>();
            match ev.id().as_ref() {
                "show" => show_main(app),
                "panic" => {
                    if let Some(e) = &state.engine {
                        e.send(Cmd::Panic);
                    }
                }
                "release" => {
                    let paused = !state.hub.status().serial_paused;
                    state.device.set_paused(&state.hub, paused);
                    let _ = state.release_item.set_checked(paused);
                }
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, ev| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev
            {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(release)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .setup(|app| {
            let handle = app.handle().clone();
            let hub = Arc::new(Hub::new(handle.clone()));

            let engine = match app
                .path()
                .resolve(SOUNDFONT, BaseDirectory::Resource)
                .map_err(|e| e.to_string())
                .and_then(|p| Engine::start(hub.clone(), &p, "GeneralUser GS"))
            {
                Ok(e) => Some(e),
                Err(err) => {
                    hub.log(format!("Sound engine failed: {err}"));
                    hub.update(|s| s.engine.error = Some(err.clone()));
                    None
                }
            };
            if let Some(e) = &engine {
                midi::set_sound(&hub, e, 0, 0, 0, 0);
                midi::set_sound(&hub, e, 9, 0, 128, 0);
            }

            let midi_link = engine.as_ref().map(|e| midi::MidiLink::start(hub.clone(), e.clone()));
            let device = DeviceLink::start(
                hub.clone(),
                midi_link.unwrap_or_else(|| Arc::new(midi::MidiLink { force_reconnect: Default::default() })),
            );
            if let Some(e) = &engine {
                let data_dir = app.path().app_data_dir()?;
                let exports = app
                    .path()
                    .audio_dir()
                    .or_else(|_| app.path().home_dir().map(|h| h.join("Music")))?
                    .join("PADI");
                Studio::start(hub.clone(), e.clone(), device.clone(), &data_dir, exports);
                // Extra sound packs: bundled ones + any .sf2 you drop in <data>/packs.
                let dirs = [app.path().resolve("resources/packs", BaseDirectory::Resource).ok(), Some(data_dir.join("packs"))];
                load_packs(hub.clone(), e.clone(), dirs.into_iter().flatten().collect());
            }
            let release_item = build_tray(&handle)?;
            app.manage(AppState { hub, engine, device, release_item });

            if !std::env::args().any(|a| a == "--minimized") {
                show_main(&handle);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps PADI running in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            get_log,
            get_presets,
            set_sound,
            play_note,
            panic,
            set_volume,
            set_serial_paused,
            get_studio,
            studio_activate,
            studio_arm,
            studio_pad,
            studio_update_pad,
            studio_clear_pad,
            studio_spread,
            kit_new,
            kit_load,
            kit_rename,
            kit_duplicate,
            kit_delete,
            studio_undo,
            studio_copy_pad,
            studio_record_into,
            set_sound_params,
            save_my_sound,
            load_my_sound,
            rename_my_sound,
            delete_my_sound,
            preview_sound,
            looper_solo,
            looper_record,
            looper_cmd,
            looper_mute,
            looper_tempo,
            export_loop,
            toggle_session
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
