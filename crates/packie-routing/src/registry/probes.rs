//! Serial port listing and endpoint probes, mapped to their IPC form.

use std::time::Duration;

use packie_config::EndpointConfig;
use packie_engine::RegistryError;
use packie_ipc::checks::{ProbeReport, ProbeStep, SerialPort, SerialPortKind};
use packie_transport::Transports;

/// The serial ports the OS reports, listed on a blocking thread.
pub(super) async fn serial_ports() -> Result<Vec<SerialPort>, RegistryError> {
    let ports = tokio::task::spawn_blocking(packie_transport::list_serial_ports)
        .await
        .map_err(|_| RegistryError::Failed("The serial port listing stopped.".into()))?
        .map_err(|e| RegistryError::Failed(e.cause))?;
    Ok(ports.into_iter().map(serial_port).collect())
}

fn serial_port(port: packie_transport::SerialPortInfo) -> SerialPort {
    SerialPort {
        name: port.name,
        kind: match port.kind {
            packie_transport::SerialPortKind::Usb => SerialPortKind::Usb,
            packie_transport::SerialPortKind::Pci => SerialPortKind::Pci,
            packie_transport::SerialPortKind::Bluetooth => SerialPortKind::Bluetooth,
            packie_transport::SerialPortKind::Unknown => SerialPortKind::Unknown,
        },
        vid: port.vid,
        pid: port.pid,
        serial_number: port.serial_number,
        manufacturer: port.manufacturer,
        product: port.product,
        used_by: Vec::new(),
    }
}

/// Probes `endpoint` through the registry's transports.
pub(super) async fn probe(
    transports: &dyn Transports,
    endpoint: &EndpointConfig,
    timeout: Duration,
) -> Result<ProbeReport, RegistryError> {
    let report = transports
        .probe_endpoint(endpoint, timeout)
        .await
        .ok_or_else(|| {
            RegistryError::NotImplemented(
                "Endpoint tests are not available with these transports.".into(),
            )
        })?;
    Ok(ProbeReport {
        ok: report.ok,
        steps: report
            .steps
            .into_iter()
            .map(|s| ProbeStep {
                name: s.name,
                ok: s.ok,
                detail: s.detail,
                millis: s.millis,
            })
            .collect(),
        summary: report.summary,
    })
}
