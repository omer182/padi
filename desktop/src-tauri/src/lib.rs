mod device;
mod engine;
mod hub;
mod midi;
mod sounds;

use device::DeviceLink;
use engine::{Cmd, Engine, PresetInfo};
use hub::{Hub, Status};
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

#[tauri::command]
fn get_presets(state: State<AppState>) -> Vec<PresetInfo> {
    state.engine.as_ref().map(|e| e.presets().to_vec()).unwrap_or_default()
}

#[tauri::command]
fn set_sound(state: State<AppState>, bank: i32, program: i32) {
    let Some(e) = &state.engine else { return };
    // Drum kits live on the GM drum channel, which the pad only plays in its
    // Drums mode. So picking a kit puts the pad in Drums mode, and picking a
    // normal sound takes it back out (to Scale mode).
    let kit = bank == 128;
    midi::set_sound(&state.hub, e, if kit { 9 } else { 0 }, bank, program);
    let in_drums = state.hub.status().device.get("st") == Some(&DRUMS_STYLE);
    if kit != in_drums {
        let cmd = if kit { format!("st {DRUMS_STYLE}") } else { "st 0".to_string() };
        if !state.device.send(&cmd) && kit {
            state.hub.log("Pad not linked: switch it to Drums mode (menu > Scales > pad 9) to play the kit");
        }
    }
}

/// Firmware PlayStyle value for Drums mode.
const DRUMS_STYLE: i32 = 5;

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
                .and_then(|p| Engine::start(hub.clone(), &p))
            {
                Ok(e) => Some(e),
                Err(err) => {
                    hub.log(format!("Sound engine failed: {err}"));
                    hub.update(|s| s.engine.error = Some(err.clone()));
                    None
                }
            };
            if let Some(e) = &engine {
                midi::set_sound(&hub, e, 0, 0, 0);
                midi::set_sound(&hub, e, 9, 128, 0);
            }

            let midi_link = engine.as_ref().map(|e| midi::MidiLink::start(hub.clone(), e.clone()));
            let device = DeviceLink::start(
                hub.clone(),
                midi_link.unwrap_or_else(|| Arc::new(midi::MidiLink { force_reconnect: Default::default() })),
            );
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
            set_serial_paused
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
