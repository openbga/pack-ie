//! `packie endpoints` and `packie notify`: setup checks against a running engine. Listing serial
//! ports and probing an endpoint never send data; `notify test` sends one test message to
//! each configured notification target.

use clap::{Args, Subcommand};
use packie_core::EndpointKind;
use packie_ipc::checks::{EndpointTest, NotificationResult, ProbeReport, SerialPort};
use packie_ipc::{Request, Response};

use crate::Failure;
use crate::client::{Target, ask, unexpected};
use crate::output::{print_json, print_line, print_table};

#[derive(Debug, Subcommand)]
pub(crate) enum EndpointsCommand {
    /// List the serial ports of this computer and the configured endpoints that use them.
    ListSerial {
        /// Print the ports as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Check an endpoint without sending data: open a serial port, connect, bind, read a
    /// folder, or log in to an SFTP server and list its directory.
    Test(TestArgs),
}

#[derive(Debug, Args)]
pub(crate) struct TestArgs {
    /// A configured endpoint to test with its active settings.
    #[arg(required_unless_present = "kind", conflicts_with_all = ["kind", "settings"])]
    endpoint: Option<String>,
    /// The kind of an unsaved definition: serial, tcp_client, tcp_listener, folder, sftp.
    #[arg(long, requires = "settings")]
    kind: Option<String>,
    /// Its settings as JSON, written as in the runtime document.
    #[arg(long, requires = "kind")]
    settings: Option<String>,
    /// The overall limit in seconds (1 to 60, default 10).
    #[arg(long)]
    timeout: Option<u32>,
    /// Print the report as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Subcommand)]
pub(crate) enum NotifyCommand {
    /// Send a test message to every configured notification target.
    Test {
        /// Print the results as JSON.
        #[arg(long)]
        json: bool,
    },
}

pub(crate) async fn endpoints(target: &Target, command: EndpointsCommand) -> Result<(), Failure> {
    match command {
        EndpointsCommand::ListSerial { json } => list_serial(target, json).await,
        EndpointsCommand::Test(args) => test_endpoint(target, args).await,
    }
}

async fn list_serial(target: &Target, json: bool) -> Result<(), Failure> {
    let client = target.connect().await?;
    let Response::SerialPorts { ports } = ask(&client, Request::ListSerialPorts).await? else {
        return Err(unexpected());
    };
    if json {
        return print_json(&ports);
    }
    if ports.is_empty() {
        print_line("The operating system reports no serial ports.");
        return Ok(());
    }
    let rows: Vec<Vec<String>> = ports.iter().map(port_row).collect();
    print_table(&["PORT", "KIND", "VID:PID", "DEVICE", "USED BY"], &rows);
    Ok(())
}

fn port_row(port: &SerialPort) -> Vec<String> {
    let ids = match (port.vid, port.pid) {
        (Some(vid), Some(pid)) => format!("{vid:04x}:{pid:04x}"),
        _ => "-".to_owned(),
    };
    let device = [port.manufacturer.as_deref(), port.product.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    let used: Vec<String> = port
        .used_by
        .iter()
        .map(|u| match u.held_by {
            Some(channel) => format!("{} (open by {channel})", u.endpoint_id),
            None => u.endpoint_id.to_string(),
        })
        .collect();
    vec![
        port.name.clone(),
        format!("{:?}", port.kind).to_lowercase(),
        ids,
        if device.is_empty() {
            "-".into()
        } else {
            device
        },
        if used.is_empty() {
            "-".into()
        } else {
            used.join(", ")
        },
    ]
}

async fn test_endpoint(target: &Target, args: TestArgs) -> Result<(), Failure> {
    let client = target.connect().await?;
    let test = match (&args.endpoint, &args.kind, &args.settings) {
        (Some(id), _, _) => configured(&client, id).await?,
        (None, Some(kind), Some(settings)) => EndpointTest {
            kind: parse_kind(kind)?,
            settings: serde_json::from_str(settings)
                .map_err(|e| Failure::invalid(format!("--settings is not JSON: {e}")))?,
            timeout_seconds: None,
        },
        _ => {
            return Err(Failure::invalid(
                "name an endpoint, or give --kind and --settings",
            ));
        }
    };
    let request = Request::TestEndpoint(EndpointTest {
        timeout_seconds: args.timeout,
        ..test
    });
    let Response::EndpointTested(report) = ask(&client, request).await? else {
        return Err(unexpected());
    };
    if args.json {
        print_json(&report)?;
    } else {
        print_report(&report);
    }
    if report.ok {
        Ok(())
    } else {
        Err(Failure::new("the endpoint test failed"))
    }
}

/// The active definition of endpoint `id`.
async fn configured(client: &packie_ipc::IpcClient, id: &str) -> Result<EndpointTest, Failure> {
    let Response::Config { document } = ask(client, Request::GetConfig).await? else {
        return Err(unexpected());
    };
    let endpoint = document
        .endpoints
        .iter()
        .find(|e| e.id.as_str() == id)
        .ok_or_else(|| Failure::new(format!("no endpoint {id} in the active configuration")))?;
    let transport =
        serde_json::to_value(&endpoint.transport).map_err(|e| Failure::new(e.to_string()))?;
    Ok(EndpointTest {
        kind: endpoint.kind(),
        settings: transport.get("settings").cloned().unwrap_or_default(),
        timeout_seconds: None,
    })
}

fn parse_kind(kind: &str) -> Result<EndpointKind, Failure> {
    EndpointKind::ALL
        .iter()
        .copied()
        .find(|k| k.as_str() == kind)
        .ok_or_else(|| {
            let kinds: Vec<&str> = EndpointKind::ALL.iter().map(|k| k.as_str()).collect();
            Failure::invalid(format!("--kind must be one of {}", kinds.join(", ")))
        })
}

fn print_report(report: &ProbeReport) {
    let rows: Vec<Vec<String>> = report
        .steps
        .iter()
        .map(|s| {
            vec![
                s.name.clone(),
                if s.ok { "ok" } else { "failed" }.to_owned(),
                format!("{} ms", s.millis),
                s.detail.clone(),
            ]
        })
        .collect();
    print_table(&["STEP", "RESULT", "TIME", "DETAIL"], &rows);
    print_line(&report.summary);
}

pub(crate) async fn notify(target: &Target, command: NotifyCommand) -> Result<(), Failure> {
    let NotifyCommand::Test { json } = command;
    let client = target.connect().await?;
    let Response::NotificationTested { results } = ask(&client, Request::TestNotification).await?
    else {
        return Err(unexpected());
    };
    if json {
        print_json(&results)?;
    } else if results.is_empty() {
        print_line(
            "No notification target is configured; add a `notifications` section to the \
             bootstrap file and restart the engine.",
        );
    } else {
        let rows: Vec<Vec<String>> = results.iter().map(result_row).collect();
        print_table(&["TARGET", "RESULT", "DETAIL"], &rows);
    }
    if results.iter().all(|r| r.ok) {
        Ok(())
    } else {
        Err(Failure::new("a test message was not delivered"))
    }
}

fn result_row(result: &NotificationResult) -> Vec<String> {
    vec![
        result.target.clone(),
        if result.ok { "sent" } else { "failed" }.to_owned(),
        result.detail.clone(),
    ]
}
