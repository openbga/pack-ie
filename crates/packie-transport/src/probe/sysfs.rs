//! Serial port listing on Linux from sysfs metadata. Nothing under `/dev` is opened, no
//! termios is touched and no port is probed: only attribute files and links below
//! `/sys/class/tty` are read.

use std::io;
use std::path::{Path, PathBuf};

use super::serial::{SerialPortInfo, SerialPortKind};

/// Resolves a sysfs link (`.../device`, `.../subsystem`) to the directory it names.
type Resolve = Box<dyn Fn(&Path) -> io::Result<PathBuf>>;

/// Where the tty class, the device nodes and the links live, so a test can point the
/// parser at a fake tree.
pub(super) struct Sysfs {
    tty_class: PathBuf,
    dev_dir: PathBuf,
    resolve: Resolve,
}

impl Sysfs {
    /// The running system.
    #[cfg(target_os = "linux")]
    pub(super) fn system() -> Self {
        Self::new(
            "/sys/class/tty",
            "/dev",
            Box::new(|link| std::fs::canonicalize(link)),
        )
    }

    pub(super) fn new(
        tty_class: impl Into<PathBuf>,
        dev_dir: impl Into<PathBuf>,
        resolve: Resolve,
    ) -> Self {
        Self {
            tty_class: tty_class.into(),
            dev_dir: dev_dir.into(),
            resolve,
        }
    }

    /// Every tty with a backing device (or an RFCOMM name) and a device node, unsorted.
    /// A system without the tty class has no serial ports.
    pub(super) fn list(&self) -> io::Result<Vec<SerialPortInfo>> {
        let entries = match std::fs::read_dir(&self.tty_class) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut ports = Vec::new();
        for entry in entries.flatten() {
            if let Some(info) = self.port(&entry.file_name().to_string_lossy()) {
                ports.push(info);
            }
        }
        Ok(ports)
    }

    fn port(&self, name: &str) -> Option<SerialPortInfo> {
        let node = self.dev_dir.join(name);
        if !node.exists() {
            return None;
        }
        let mut info = SerialPortInfo {
            name: node.to_string_lossy().into_owned(),
            kind: SerialPortKind::Unknown,
            vid: None,
            pid: None,
            serial_number: None,
            manufacturer: None,
            product: None,
        };
        let dir = self.tty_class.join(name);
        if name.starts_with("rfcomm") {
            info.kind = SerialPortKind::Bluetooth;
            return Some(info);
        }
        let device = (self.resolve)(&dir.join("device")).ok()?;
        // The kernel registers every legacy 8250 slot as a platform port; the ones without
        // a UART report port type 0 ("unknown") and are not real ports.
        if read_trimmed(&dir.join("type")).as_deref() == Some("0") {
            return None;
        }
        self.classify(&device, &mut info);
        Some(info)
    }

    /// The nearest ancestor of the tty's device that is a USB device or a PCI device
    /// decides the kind; a USB device is found before the PCI host controller above it.
    fn classify(&self, device: &Path, info: &mut SerialPortInfo) {
        for dir in device.ancestors() {
            if dir.join("idVendor").is_file() {
                info.kind = SerialPortKind::Usb;
                info.vid = read_hex(&dir.join("idVendor"));
                info.pid = read_hex(&dir.join("idProduct"));
                info.serial_number = read_trimmed(&dir.join("serial"));
                info.manufacturer = read_trimmed(&dir.join("manufacturer"));
                info.product = read_trimmed(&dir.join("product"));
                return;
            }
            if self.is_pci(dir) {
                info.kind = SerialPortKind::Pci;
                return;
            }
        }
    }

    fn is_pci(&self, dir: &Path) -> bool {
        (self.resolve)(&dir.join("subsystem"))
            .ok()
            .and_then(|bus| bus.file_name().map(|n| n == "pci"))
            .unwrap_or(false)
    }
}

fn read_trimmed(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn read_hex(path: &Path) -> Option<u16> {
    u16::from_str_radix(&read_trimmed(path)?, 16).ok()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// A fake sysfs and `/dev` in a temp dir. A link is a plain file holding its target
    /// relative to the root, so the tree builds without symlink rights.
    struct Fake {
        root: tempfile::TempDir,
    }

    impl Fake {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            fs::create_dir_all(root.path().join("sys/class/tty")).unwrap();
            fs::create_dir_all(root.path().join("dev")).unwrap();
            Self { root }
        }

        fn path(&self, rel: &str) -> PathBuf {
            self.root.path().join(rel)
        }

        fn file(&self, rel: &str, text: &str) {
            let path = self.path(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }

        /// A tty `name` whose device link names `device`, with a `/dev` node.
        fn tty(&self, name: &str, device: &str) {
            self.file(&format!("sys/class/tty/{name}/device"), device);
            fs::create_dir_all(self.path(device)).unwrap();
            self.file(&format!("dev/{name}"), "");
        }

        fn sysfs(&self) -> Sysfs {
            let root = self.root.path().to_owned();
            let resolve = move |link: &Path| {
                let target = fs::read_to_string(link)?;
                Ok(root.join(target.trim()))
            };
            Sysfs::new(
                self.path("sys/class/tty"),
                self.path("dev"),
                Box::new(resolve),
            )
        }

        fn list(&self) -> Vec<SerialPortInfo> {
            let mut ports = self.sysfs().list().unwrap();
            ports.sort_by(|a, b| a.name.cmp(&b.name));
            ports
        }

        fn name(&self, tty: &str) -> String {
            self.path(&format!("dev/{tty}"))
                .to_string_lossy()
                .into_owned()
        }
    }

    #[test]
    fn usb_adapter_reports_vendor_product_and_texts() {
        let fake = Fake::new();
        let usb = "sys/devices/pci0000:00/0000:00:14.0/usb1/1-2";
        fake.file(&format!("{usb}/idVendor"), "0403\n");
        fake.file(&format!("{usb}/idProduct"), "6001\n");
        fake.file(&format!("{usb}/serial"), "A50285BI\n");
        fake.file(&format!("{usb}/manufacturer"), "FTDI\n");
        fake.file(&format!("{usb}/product"), "FT232R USB UART\n");
        // The host controller above the USB device is PCI; the USB device must win.
        fake.file(
            "sys/devices/pci0000:00/0000:00:14.0/subsystem",
            "sys/bus/pci",
        );
        fake.tty("ttyUSB0", &format!("{usb}/1-2:1.0/ttyUSB0"));
        let ports = fake.list();
        assert_eq!(
            ports,
            [SerialPortInfo {
                name: fake.name("ttyUSB0"),
                kind: SerialPortKind::Usb,
                vid: Some(0x0403),
                pid: Some(0x6001),
                serial_number: Some("A50285BI".to_owned()),
                manufacturer: Some("FTDI".to_owned()),
                product: Some("FT232R USB UART".to_owned()),
            }]
        );
    }

    #[test]
    fn platform_8250_port_is_unknown_and_empty_slots_are_skipped() {
        let fake = Fake::new();
        fake.tty("ttyS0", "sys/devices/platform/serial8250");
        fake.file("sys/class/tty/ttyS0/type", "4\n");
        fake.file("sys/devices/platform/subsystem", "sys/bus/platform");
        fake.tty("ttyS1", "sys/devices/platform/serial8250");
        fake.file("sys/class/tty/ttyS1/type", "0\n");
        let ports = fake.list();
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].name, fake.name("ttyS0"));
        assert_eq!(ports[0].kind, SerialPortKind::Unknown);
        assert_eq!(ports[0].vid, None);
    }

    #[test]
    fn pci_port_is_pci() {
        let fake = Fake::new();
        let pci = "sys/devices/pci0000:00/0000:00:16.3";
        fake.file(&format!("{pci}/subsystem"), "sys/bus/pci");
        fake.tty("ttyS4", &format!("{pci}/tty/ttyS4/serial0"));
        fake.file("sys/class/tty/ttyS4/type", "4\n");
        let ports = fake.list();
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].kind, SerialPortKind::Pci);
    }

    #[test]
    fn rfcomm_is_bluetooth_and_ttys_without_device_or_node_are_skipped() {
        let fake = Fake::new();
        fake.file("sys/class/tty/rfcomm0/uevent", "");
        fake.file("dev/rfcomm0", "");
        // A virtual console: no backing device.
        fake.file("sys/class/tty/tty1/uevent", "");
        fake.file("dev/tty1", "");
        // A backing device but no device node.
        fake.file("sys/class/tty/ttyUSB9/device", "sys/devices/gone");
        let ports = fake.list();
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].kind, SerialPortKind::Bluetooth);
    }

    #[test]
    fn missing_tty_class_lists_nothing() {
        let fake = Fake::new();
        fs::remove_dir_all(fake.path("sys")).unwrap();
        assert!(fake.list().is_empty());
    }
}
