//! Serial ports: listing what the OS reports, and probing a serial endpoint.

use std::path::Path;
use std::time::Duration;

use packie_config::SerialSettings;

use super::{PortUse, Run};
use crate::folder::blocking;
use crate::link::{LinkError, Operation};

/// What kind of bus a serial port sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerialPortKind {
    /// A USB serial adapter or device.
    Usb,
    /// A PCI or built-in port.
    Pci,
    /// A Bluetooth serial port.
    Bluetooth,
    /// The OS did not say.
    Unknown,
}

impl SerialPortKind {
    /// `usb`, `pci`, `bluetooth` or `unknown`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Usb => "usb",
            Self::Pci => "pci",
            Self::Bluetooth => "bluetooth",
            Self::Unknown => "unknown",
        }
    }
}

/// One serial port the OS reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialPortInfo {
    /// `COM3`, `/dev/ttyUSB0`.
    pub name: String,
    /// The bus.
    pub kind: SerialPortKind,
    /// USB vendor id.
    pub vid: Option<u16>,
    /// USB product id.
    pub pid: Option<u16>,
    /// USB serial number of the adapter.
    pub serial_number: Option<String>,
    /// USB manufacturer text.
    pub manufacturer: Option<String>,
    /// USB product text.
    pub product: Option<String>,
}

/// The serial ports the operating system reports, sorted by name. Listing opens nothing.
///
/// # Errors
///
/// The operating system could not enumerate ports.
pub fn list_serial_ports() -> Result<Vec<SerialPortInfo>, LinkError> {
    let mut ports = enumerate()?;
    ports.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(ports)
}

fn list_error(cause: impl std::fmt::Display) -> LinkError {
    LinkError::new(
        Operation::Open,
        format!("cannot list serial ports: {cause}"),
    )
}

/// Linux reads sysfs only: the enumeration in `serialport` opens `serial8250` devices to
/// decide whether they exist.
#[cfg(target_os = "linux")]
fn enumerate() -> Result<Vec<SerialPortInfo>, LinkError> {
    super::sysfs::Sysfs::system().list().map_err(list_error)
}

#[cfg(not(target_os = "linux"))]
fn enumerate() -> Result<Vec<SerialPortInfo>, LinkError> {
    let ports = serialport::available_ports().map_err(|e| list_error(e.description))?;
    Ok(ports.into_iter().map(info).collect())
}

#[cfg(not(target_os = "linux"))]
fn info(port: serialport::SerialPortInfo) -> SerialPortInfo {
    let mut out = SerialPortInfo {
        name: port.port_name,
        kind: SerialPortKind::Unknown,
        vid: None,
        pid: None,
        serial_number: None,
        manufacturer: None,
        product: None,
    };
    match port.port_type {
        serialport::SerialPortType::UsbPort(usb) => {
            out.kind = SerialPortKind::Usb;
            out.vid = Some(usb.vid);
            out.pid = Some(usb.pid);
            out.serial_number = usb.serial_number;
            out.manufacturer = usb.manufacturer;
            out.product = usb.product;
        }
        serialport::SerialPortType::PciPort => out.kind = SerialPortKind::Pci,
        serialport::SerialPortType::BluetoothPort => out.kind = SerialPortKind::Bluetooth,
        serialport::SerialPortType::Unknown => {}
    }
    out
}

/// A port this process uses is reported without opening it; otherwise the port is opened
/// with the configured settings and closed again, never read or written. A virtual endpoint
/// only reports what would be created.
pub(super) async fn probe(run: &mut Run, settings: &SerialSettings, usage: PortUse) {
    let name = settings
        .virtual_link
        .as_ref()
        .map_or_else(|| settings.port_name.clone(), |l| l.display().to_string());
    let step = match settings.virtual_link {
        Some(_) => "virtual link",
        None => "open",
    };
    match usage {
        PortUse::Held(holder) => {
            let detail = format!("in use by this engine (channel {holder})");
            return run.note(step, false, detail, Duration::ZERO);
        }
        PortUse::OpenIdle => {
            let detail = "in use by this engine (open between connection attempts)";
            return run.note(step, false, detail, Duration::ZERO);
        }
        PortUse::Free => {}
    }
    let ok = match &settings.virtual_link {
        Some(link) => {
            let link = link.clone();
            run.step(step, blocking(move || virtual_link(&link)))
                .await
                .is_some()
        }
        None => {
            let owned = settings.clone();
            let open = blocking(move || physical(&owned));
            run.step(step, open).await.is_some()
        }
    };
    if ok {
        run.succeeded(format!("{name} can be used with these settings."));
    }
}

fn physical(settings: &SerialSettings) -> Result<((), String), LinkError> {
    crate::serial::probe_open(settings)?;
    Ok((
        (),
        format!(
            "{} opened at {} baud and closed again; nothing was sent",
            settings.port_name, settings.baud_rate
        ),
    ))
}

#[cfg(unix)]
fn virtual_link(link: &Path) -> Result<((), String), LinkError> {
    let fail = |cause: String| LinkError::persistent(Operation::Open, cause);
    if let Ok(meta) = std::fs::symlink_metadata(link)
        && !meta.file_type().is_symlink()
    {
        return Err(fail(format!("{} exists and is not a link", link.display())));
    }
    let parent = link.parent().filter(|p| !p.as_os_str().is_empty());
    if let Some(parent) = parent
        && !parent.is_dir()
    {
        return Err(fail(format!(
            "directory {} does not exist",
            parent.display()
        )));
    }
    Ok((
        (),
        format!(
            "the engine would create a pseudo-terminal pair and expose one end at {}; \
             nothing was created",
            link.display()
        ),
    ))
}

#[cfg(not(unix))]
fn virtual_link(_link: &Path) -> Result<((), String), LinkError> {
    Err(LinkError::persistent(
        Operation::Open,
        "virtual serial links need Unix",
    ))
}
