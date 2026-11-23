use std::ffi::c_void;

use tauri::Emitter;

// CoreAudio/AudioHardware.h
#[repr(C)]
struct AudioObjectPropertyAddress {
    selector: u32,
    scope: u32,
    element: u32,
}

const SYSTEM_OBJECT: u32 = 1; // kAudioObjectSystemObject
const DEVICES_ADDRESS: AudioObjectPropertyAddress = AudioObjectPropertyAddress {
    selector: u32::from_be_bytes(*b"dev#"), // kAudioHardwarePropertyDevices
    scope: u32::from_be_bytes(*b"glob"),    // kAudioObjectPropertyScopeGlobal
    element: 0,                             // kAudioObjectPropertyElementMain
};

type Listener = extern "C" fn(u32, u32, *const AudioObjectPropertyAddress, *mut c_void) -> i32;

#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioObjectAddPropertyListener(
        object_id: u32,
        address: *const AudioObjectPropertyAddress,
        listener: Listener,
        client_data: *mut c_void,
    ) -> i32;
}

// Runs on the CoreAudio HAL notification thread. It only reports that the device
// list changed; the frontend enumerates devices again through `list_devices`.
extern "C" fn on_devices_changed(
    _object_id: u32,
    _num_addresses: u32,
    _addresses: *const AudioObjectPropertyAddress,
    client_data: *mut c_void,
) -> i32 {
    // SAFETY: `start` passes a leaked `AppHandle` that stays valid until the process exits.
    let app = unsafe { &*(client_data as *const tauri::AppHandle) };
    let _ = app.emit("echoless://devices-changed", ());
    0
}

/// Registers the device-list listener once for the lifetime of the process.
/// The listener is never removed: CoreAudio can still call it while the app
/// exits, so its `AppHandle` is leaked instead of freed.
pub fn start(app: &tauri::AppHandle) {
    let client: &'static tauri::AppHandle = Box::leak(Box::new(app.clone()));
    let status = unsafe {
        AudioObjectAddPropertyListener(
            SYSTEM_OBJECT,
            &DEVICES_ADDRESS,
            on_devices_changed,
            client as *const tauri::AppHandle as *mut c_void,
        )
    };
    if status != 0 {
        let message = format!("device watch: AudioObjectAddPropertyListener failed ({status})");
        crate::logging::log("error", "device-watch", &message);
        let _ = app.emit("echoless://log", message);
    }
}
