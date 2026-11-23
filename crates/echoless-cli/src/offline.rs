use anyhow::Result;

use crate::cli::OfflineArgs;
use crate::config_validate::ensure_valid_pipeline_config;
use echoless_audio_io::file::{WavFileSink, WavFileSource};
use echoless_core::{run_offline, DiagnosticsConfig, PipelineConfig};
use echoless_processors::NodeConfig;

fn offline_pipeline_config(a: &OfflineArgs) -> Result<PipelineConfig> {
    let mut cfg = match &a.config {
        Some(path) => toml::from_str(&std::fs::read_to_string(path)?)?,
        None => PipelineConfig {
            sample_rate: a.rate,
            frame_ms: a.frame_ms,
            chain: a
                .chain
                .as_deref()
                .unwrap_or("")
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|k| NodeConfig {
                    kind: k.to_string(),
                    params: toml::Table::new(),
                })
                .collect(),
            ..PipelineConfig::default()
        },
    };
    cfg.mic = a.mic.clone();
    cfg.reference = a.reference.clone();
    cfg.output = a.out.clone();
    if let Some(level) = a.output_level {
        cfg.output_level = level;
    }
    // Offline inputs (e.g. diagnostics mic.wav) already carry the near delay.
    cfg.near_delay_ms = 0;
    cfg.bypass = false;
    cfg.diagnostics = DiagnosticsConfig::default();
    Ok(cfg)
}

pub(crate) fn cmd_offline(a: OfflineArgs) -> Result<()> {
    let cfg = offline_pipeline_config(&a)?;
    ensure_valid_pipeline_config(&cfg)?;

    let frame = cfg.frame_size();
    let mic = WavFileSource::new(&a.mic, frame)?;
    let reference = WavFileSource::new(&a.reference, frame)?;
    let sink = WavFileSink::new(&a.out);

    let chain_desc = if cfg.chain.is_empty() {
        "passthrough".to_string()
    } else {
        cfg.chain
            .iter()
            .map(|n| n.kind.clone())
            .collect::<Vec<_>>()
            .join(" -> ")
    };
    println!("offline run: {} + {} -> {}", a.mic, a.reference, a.out);
    println!(
        "sample_rate {} Hz · frame {} ms · reference={} · output_level={} · chain: {}",
        cfg.sample_rate,
        cfg.frame_ms,
        cfg.reference_channels.as_str(),
        cfg.output_level,
        chain_desc
    );

    let rep = run_offline(&cfg, mic, reference, sink)?;
    println!(
        "done: {} frames (~{:.2}s) · chain [{}] · total algorithmic latency {:.1} ms",
        rep.frames,
        rep.seconds,
        rep.chain.join(", "),
        rep.total_latency_ms
    );
    for s in &rep.node_stats {
        println!(
            "  - {}: ERLE {:.1} dB, delay {} ms, process {:.2} ms, runtime_errors={}, diverged={}",
            s.name,
            s.erle_db,
            s.estimated_delay_ms,
            s.process_time_ms,
            s.runtime_error_count,
            s.diverged
        );
        if let Some(model) = &s.selected_model {
            println!("      model={model}");
        }
        if let Some(err) = &s.last_backend_error {
            println!("      last_error={err}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use echoless_core::ReferenceChannels;

    fn offline_args(config: Option<String>) -> OfflineArgs {
        OfflineArgs {
            mic: "mic.wav".into(),
            reference: "ref.wav".into(),
            out: "out.wav".into(),
            config,
            chain: Some("aec3".into()),
            rate: 48_000,
            frame_ms: 10,
            output_level: None,
        }
    }

    #[test]
    fn offline_config_keeps_stereo_reference_and_drops_near_delay() {
        let dir = std::env::temp_dir().join(format!("echoless-offline-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("stereo.toml");
        std::fs::write(
            &path,
            "reference_channels = \"stereo\"\nnear_delay_ms = 40\n\n[[chain]]\nkind = \"aec3\"\n",
        )
        .unwrap();

        let cfg = offline_pipeline_config(&offline_args(Some(path.display().to_string()))).unwrap();

        assert_eq!(cfg.reference_channels, ReferenceChannels::Stereo);
        assert_eq!(cfg.near_delay_ms, 0);
        assert_eq!(cfg.mic, "mic.wav");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn offline_shortcut_chain_uses_mono_reference() {
        let cfg = offline_pipeline_config(&offline_args(None)).unwrap();

        assert_eq!(cfg.reference_channels, ReferenceChannels::Mono);
        assert_eq!(cfg.chain.len(), 1);
        assert_eq!(cfg.chain[0].kind, "aec3");
    }
}
