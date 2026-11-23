use anyhow::Result;

use crate::cli::{DevicesArgs, DoctorArgs, DoctorAudioArgs, DoctorCmd};
use crate::realtime;

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

pub(crate) fn cmd_doctor(args: DoctorArgs) -> Result<()> {
    match args.cmd {
        DoctorCmd::Audio(a) => cmd_doctor_audio(a),
    }
}

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
