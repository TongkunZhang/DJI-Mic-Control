//! `djimic` — get and set DJI microphone settings from the command line.
//!
//! One-shot by design: each invocation connects, performs the action, prints the
//! result, and exits. For a live view, run repeatedly or use the GUI.

use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use device::{DeviceManager, DeviceStatus};
use device::protocol::TxInfo;

/// How long to wait for a device to appear and send its first heartbeat.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Parser)]
#[command(name = "djimic", about = "Control DJI wireless microphones.", version)]
struct Cli {
    /// Target device id (its serial). Optional when exactly one is connected.
    #[arg(long, short, global = true)]
    device: Option<String>,

    /// Emit machine-readable JSON where applicable.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List connected devices.
    List,
    /// Show a device's full status.
    Status,
    /// Read one setting's current value.
    Get {
        /// Setting id, e.g. `noise-cancel`.
        setting: String,
    },
    /// Change one setting.
    Set {
        /// Setting id, e.g. `noise-cancel`.
        setting: String,
        /// Value slug, e.g. `strong`.
        value: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mgr = DeviceManager::new();
    mgr.refresh();

    match cli.command {
        Command::List => list(&mgr, cli.json),
        Command::Status => {
            let id = resolve(&mgr, cli.device.as_deref())?;
            let status = wait_connected(&mgr, &id)?;
            print_status(&mgr, &id, &status, cli.json)
        }
        Command::Get { setting } => {
            let id = resolve(&mgr, cli.device.as_deref())?;
            let status = wait_connected(&mgr, &id)?;
            get(&status, &setting, cli.json)
        }
        Command::Set { setting, value } => {
            let id = resolve(&mgr, cli.device.as_deref())?;
            wait_connected(&mgr, &id)?;
            mgr.set(&id, &setting, &value)
                .with_context(|| format!("setting {setting} = {value}"))?;
            println!("{setting} = {value}  (sent)");
            Ok(())
        }
    }
}

/// Resolve the target device id, or fail with helpful guidance.
fn resolve(mgr: &DeviceManager, requested: Option<&str>) -> Result<String> {
    if let Some(id) = requested {
        return Ok(id.to_string());
    }
    if let Some(id) = mgr.only_device() {
        return Ok(id);
    }

    let probe = mgr.probe();
    let devices = mgr.list();
    if devices.is_empty() {
        if probe.permission_issue {
            print_udev_help();
            bail!("a microphone is connected but not accessible");
        }
        bail!("no microphone detected — check the USB connection");
    }
    let ids: Vec<String> = devices.iter().map(|d| d.id.clone()).collect();
    bail!("multiple devices connected; pass --device <id>. Available: {}", ids.join(", "));
}

/// Poll until the device reports a decoded heartbeat, or time out.
fn wait_connected(mgr: &DeviceManager, id: &str) -> Result<DeviceStatus> {
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    loop {
        if let Ok(status) = mgr.status(id) {
            if status.connected {
                return Ok(status);
            }
        }
        if Instant::now() >= deadline {
            bail!("device {id} did not report status in time");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn list(mgr: &DeviceManager, json: bool) -> Result<()> {
    // Give freshly-attached devices a moment to announce themselves.
    let deadline = Instant::now() + Duration::from_millis(600);
    while mgr.list().is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let devices = mgr.list();

    if json {
        println!("{}", serde_json::to_string_pretty(&devices)?);
        return Ok(());
    }

    if devices.is_empty() {
        let probe = mgr.probe();
        if probe.permission_issue {
            print_udev_help();
        } else {
            println!("No devices connected.");
        }
        return Ok(());
    }
    for d in devices {
        let serial = d.rx_serial.unwrap_or_else(|| "—".into());
        let state = if d.connected { "connected" } else { "connecting" };
        println!("{}  {}  rx:{}  [{}]", d.id, d.model_name, serial, state);
    }
    Ok(())
}

fn get(status: &DeviceStatus, setting: &str, json: bool) -> Result<()> {
    let value = status
        .settings
        .get(setting)
        .with_context(|| format!("unknown or unreported setting {setting:?}"))?;
    if json {
        println!("{}", serde_json::json!({ setting: value }));
    } else {
        println!("{value}");
    }
    Ok(())
}

/// Render an optional identity/level field, e.g. a TX's serial when the
/// connected firmware hasn't reported it yet (see `PROTOCOL.md`).
fn unknown<T: std::fmt::Display>(v: &Option<T>) -> String {
    v.as_ref().map(|v| v.to_string()).unwrap_or_else(|| "unknown".to_string())
}

/// A transmitter slot's display label: its own reported product name once
/// known (e.g. `"DJI Mic Mini 2 (TX1)"`), else the bare slot number.
fn tx_label(i: usize, tx: Option<&TxInfo>) -> String {
    match tx.and_then(|t| t.product_name.as_deref()) {
        Some(name) => format!("{name} (TX{})", i + 1),
        None => format!("TX {}", i + 1),
    }
}

fn print_status(mgr: &DeviceManager, id: &str, status: &DeviceStatus, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(status)?);
        return Ok(());
    }

    println!("Device:  {id}  ({})", status.model_id);
    if let Some(rx) = &status.rx {
        println!("Receiver: serial {}  fw {}", unknown(&rx.serial), unknown(&rx.firmware));
    }
    for (i, tx) in status.tx.iter().enumerate() {
        let label = tx_label(i, tx.as_ref());
        match tx {
            Some(t) => println!(
                "{label}:      serial {}  fw {}  level {}",
                unknown(&t.serial),
                unknown(&t.firmware),
                unknown(&t.level)
            ),
            None => println!("{label}:      (off)"),
        }
    }
    println!("NC button: {}", if status.nc_enabled { "on" } else { "off" });

    println!("\nSettings:");
    // Present them in the model's declared order where possible.
    if let Ok(defs) = mgr.settings(id) {
        for s in defs {
            if let Some(v) = status.settings.get(s.id) {
                println!("  {:<22} {}", s.label, v);
            }
        }
    } else {
        for (k, v) in &status.settings {
            println!("  {k:<22} {v}");
        }
    }
    Ok(())
}

fn print_udev_help() {
    eprintln!("A supported microphone is connected but cannot be accessed.");
    eprintln!("On Linux this is usually missing udev rules.\n");
    for step in device::udev_instructions() {
        eprintln!("  • {step}");
    }
    eprintln!("\nRule to install at {}:\n", device::UDEV_RULE_FILE);
    eprint!("{}", device::udev_rule());
}
