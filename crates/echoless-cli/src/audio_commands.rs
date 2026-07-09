use anyhow::Result;
#[cfg(not(feature = "realtime"))]
use serde_json::json;

use crate::cli::{DevicesArgs, DoctorArgs, DoctorAudioArgs, DoctorCmd};
#[cfg(feature = "realtime")]
use crate::realtime;

#[cfg(feature = "realtime")]
pub(crate) fn cmd_devices(args: DevicesArgs) -> Result<()> {
    if args.json {
        let devices = if args.fast {
            realtime::devices_json_with_options(realtime::DeviceListOptions {
                include_config_details: false,
            })?
        } else {
            realtime::devices_json()?
        };
        println!("{}", serde_json::to_string_pretty(&devices)?);
        return Ok(());
    }
    realtime::print_devices()
}

#[cfg(not(feature = "realtime"))]
pub(crate) fn cmd_devices(args: DevicesArgs) -> Result<()> {
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "ok": false,
                "error": "device enumeration requires the realtime feature (cpal); current build has it disabled.",
                "inputs": [],
                "outputs": [],
                "reference_sources": [
                    { "id": "system", "label": "System audio", "kind": "system" },
                    { "id": "none", "label": "No reference", "kind": "none" }
                ]
            }))?
        );
        return Ok(());
    }
    println!(
        "device enumeration requires the realtime feature (cpal); current build has it disabled."
    );
    Ok(())
}

#[cfg(feature = "realtime")]
pub(crate) fn cmd_doctor(args: DoctorArgs) -> Result<()> {
    match args.cmd {
        DoctorCmd::Audio(a) => cmd_doctor_audio(a),
    }
}

#[cfg(feature = "realtime")]
pub(crate) fn cmd_doctor_audio(args: DoctorAudioArgs) -> Result<()> {
    let report = realtime::audio_doctor_json_with_options(realtime::AudioDoctorOptions {
        include_config_details: !args.fast_devices,
        request_system_audio: args.request_system_audio,
    })?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    println!("Audio doctor");
    println!("  ok: {}", report["ok"]);
    println!(
        "  virtual_output_detected: {}",
        report["virtual_output_detected"]
    );
    println!("  recommended_driver: {}", report["recommended_driver"]);
    println!("  install_status: {}", report["install_status"]);
    println!("Use --json for GUI-readable details.");
    Ok(())
}

#[cfg(not(feature = "realtime"))]
pub(crate) fn cmd_doctor(args: DoctorArgs) -> Result<()> {
    match args.cmd {
        DoctorCmd::Audio(a) => cmd_doctor_audio(a),
    }
}

#[cfg(not(feature = "realtime"))]
pub(crate) fn cmd_doctor_audio(args: DoctorAudioArgs) -> Result<()> {
    let report = json!({
        "ok": false,
        "platform": std::env::consts::OS,
        "error": "audio doctor requires the realtime feature (cpal); current build has it disabled.",
        "virtual_output_detected": false,
        "candidate_outputs": [],
        "candidate_inputs": [],
        "recommended_driver": recommended_audio_driver(),
        "install_status": "unknown",
        "needs_reboot": false,
        "permission_state": "unknown",
        "system_audio_permission": "unknown",
        "system_audio_permission_probe": if args.request_system_audio {
            json!({
                "requested": true,
                "ok": false,
                "state": "unknown",
                "detail": "audio doctor requires the realtime feature (cpal); current build has it disabled."
            })
        } else {
            json!(null)
        },
    });
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "audio doctor requires the realtime feature (cpal); current build has it disabled."
        );
    }
    Ok(())
}

#[cfg(not(feature = "realtime"))]
fn recommended_audio_driver() -> &'static str {
    if cfg!(windows) {
        "vb-cable"
    } else if cfg!(target_os = "macos") {
        "blackhole-2ch"
    } else {
        "unknown"
    }
}
