import type {
  EndpointConfig,
  FolderEndpointConfig,
  SerialEndpointConfig,
  SftpEndpointConfig,
  TcpClientEndpointConfig,
  TcpListenerEndpointConfig,
} from "../types";

const BASE64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/** A stable, well-formed `SHA256:` fingerprint (43 base64 characters) derived from the id. */
function hostKeyFingerprint(id: string): string {
  let state = 2166136261;
  for (const char of id) state = Math.imul(state ^ char.charCodeAt(0), 16777619) >>> 0;
  let body = "";
  for (let i = 0; i < 43; i++) {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    body += BASE64.charAt(state >>> 26);
  }
  return `SHA256:${body}`;
}

type SerialOptions = Partial<Omit<SerialEndpointConfig, "kind" | "id" | "label" | "port">>;

function serial(id: string, label: string, port: string, options: SerialOptions = {}) {
  const config: SerialEndpointConfig = {
    kind: "serial",
    id,
    label,
    enabled: true,
    port,
    baudRate: 9600,
    dataBits: 8,
    parity: "none",
    stopBits: 1,
    flowControl: "none",
    ...options,
  };
  return config;
}

function tcpClient(id: string, label: string, host: string, port: number) {
  const config: TcpClientEndpointConfig = {
    kind: "tcp_client",
    id,
    label,
    enabled: true,
    host,
    port,
    connectTimeoutMs: 5000,
    idleTimeoutSeconds: 0,
    keepalive: true,
  };
  return config;
}

function tcpListener(id: string, label: string, port: number, allowlist: string[]) {
  const config: TcpListenerEndpointConfig = {
    kind: "tcp_listener",
    id,
    label,
    enabled: true,
    bindAddress: "10.0.4.5",
    port,
    allowlist,
    allowAnyPeer: false,
    idleTimeoutSeconds: 0,
    keepalive: true,
  };
  return config;
}

function folder(id: string, label: string, path: string, filePattern: string) {
  const config: FolderEndpointConfig = {
    kind: "folder",
    id,
    label,
    enabled: true,
    path,
    mode: "watch",
    pollIntervalSec: 5,
    filePattern,
  };
  return config;
}

function sftp(
  id: string,
  label: string,
  host: string,
  username: string,
  secretName: string,
  remotePath: string,
) {
  const config: SftpEndpointConfig = {
    kind: "sftp",
    id,
    label,
    enabled: true,
    host,
    port: 22,
    username,
    authMethod: secretName.endsWith("-key") ? "private_key" : "password",
    secretName,
    passphraseSecretName: "",
    hostKeyFingerprint: hostKeyFingerprint(id),
    remotePath,
    pollIntervalSec: 60,
    filePattern: "*",
  };
  return config;
}

/** Analyzer side of a bridge: a serial port or a listener the analyzer connects to. */
const BRIDGE_ANALYZERS: { slug: string; label: string; endpoint: EndpointConfig }[] = [
  {
    slug: "au5800",
    label: "AU5800 chem analyzer",
    endpoint: serial("ep-a-au5800", "AU5800 chem analyzer (COM4)", "COM4", { baudRate: 19200 }),
  },
  {
    slug: "c503",
    label: "cobas c 503",
    endpoint: serial("ep-a-c503", "cobas c 503 (COM5)", "COM5"),
  },
  {
    slug: "e801",
    label: "cobas e 801",
    endpoint: serial("ep-a-e801", "cobas e 801 (COM6)", "COM6", {
      dataBits: 7,
      parity: "even",
    }),
  },
  {
    slug: "im1600",
    label: "Atellica IM1600",
    endpoint: serial("ep-a-im1600", "Atellica IM1600 (COM7)", "COM7", { baudRate: 38400 }),
  },
  {
    slug: "v5600",
    label: "Vitros 5600",
    endpoint: serial("ep-a-v5600", "Vitros 5600 (COM8)", "COM8"),
  },
  {
    slug: "acltop",
    label: "ACL TOP 550 coag",
    endpoint: serial("ep-a-acltop", "ACL TOP 550 coag (COM9)", "COM9", {
      flowControl: "rts_cts",
    }),
  },
  {
    slug: "u411",
    label: "cobas u 411 urine",
    endpoint: serial("ep-a-u411", "cobas u 411 urine (COM10)", "COM10"),
  },
  {
    slug: "b221",
    label: "cobas b 221 gases",
    endpoint: serial("ep-a-b221", "cobas b 221 gases (COM11)", "COM11", { baudRate: 4800 }),
  },
  {
    slug: "d100",
    label: "D-100 HbA1c",
    endpoint: serial("ep-a-d100", "D-100 HbA1c (COM12)", "COM12"),
  },
  {
    slug: "xn3000",
    label: "Sysmex XN-3000",
    endpoint: tcpListener("ep-a-xn3000", "Sysmex XN-3000 (bridge)", 4001, ["10.0.4.33"]),
  },
  {
    slug: "dxh900",
    label: "DxH 900 hematology",
    endpoint: tcpListener("ep-a-dxh900", "DxH 900 hematology (bridge)", 4002, ["10.0.4.34"]),
  },
  {
    slug: "ci4100",
    label: "Architect ci4100",
    endpoint: serial("ep-a-ci4100", "Architect ci4100 (COM13)", "COM13"),
  },
];

export interface BridgeSpec {
  slug: string;
  label: string;
  endpointA: EndpointConfig;
  endpointB: TcpClientEndpointConfig;
}

export function bridgeSpecs(): BridgeSpec[] {
  return BRIDGE_ANALYZERS.map((analyzer, index) => ({
    slug: analyzer.slug,
    label: analyzer.label,
    endpointA: analyzer.endpoint,
    endpointB: tcpClient(
      `ep-mw-${index + 1}`,
      `Middleware feed ${index + 1}`,
      "10.0.4.12",
      3001 + index,
    ),
  }));
}

/** Endpoints used by message channels, in the order the channel catalog refers to them. */
export function messageEndpoints(): EndpointConfig[] {
  return [
    // Sources
    tcpListener("ep-au680", "AU680 chem analyzer", 5001, ["10.0.4.41"]),
    tcpListener("ep-xn1000", "Sysmex XN-1000", 5002, ["10.0.4.31"]),
    tcpListener("ep-xn2000", "Sysmex XN-2000", 5003, ["10.0.4.32"]),
    serial("ep-c311", "cobas c 311 chem analyzer", "COM20"),
    serial("ep-e411", "cobas e 411 immunoassay", "COM21", { dataBits: 7, parity: "even" }),
    serial("ep-i1000", "Architect i1000SR", "COM22"),
    serial("ep-vitros350", "Vitros 350 chem analyzer", "COM23"),
    serial("ep-sta", "STA Compact coag", "COM24", { baudRate: 19200 }),
    serial("ep-urisys", "Urisys 2400 urine", "COM25"),
    tcpListener("ep-istat", "i-STAT cradle gateway", 5010, ["10.0.4.0/24"]),
    serial("ep-d10", "D-10 HbA1c", "COM26"),
    folder("ep-qc-in", "QC export folder", "D:\\LabData\\QC\\Incoming", "*.csv"),
    tcpListener("ep-lis-orders", "LIS orders listener", 2576, ["10.0.4.21", "10.0.4.22"]),
    sftp(
      "ep-orders-sftp",
      "Orders folder (SFTP)",
      "sftp.partner.example",
      "labxfer",
      "orders-sftp-key",
      "/outbound/orders",
    ),
    sftp(
      "ep-reflab-sftp",
      "Reference lab SFTP",
      "sftp.reflab.example",
      "labxfer",
      "reflab-sftp-password",
      "/results/main-lab",
    ),
    tcpListener("ep-adt-in", "ADT feed listener", 2580, ["10.0.4.21"]),
    folder(
      "ep-poc-in",
      "Point-of-care export folder",
      "D:\\LabData\\POC\\Incoming",
      "glucose-*.csv",
    ),
    tcpListener("ep-abl", "ABL90 blood gas", 5021, ["10.0.4.45"]),
    tcpListener("ep-outreach-in", "Outreach HL7 listener", 2590, ["10.20.0.0/16"]),
    sftp(
      "ep-clinic-sftp",
      "Referring clinic SFTP",
      "sftp.clinic.example",
      "mainlab",
      "clinic-sftp-key",
      "/orders/pending",
    ),
    tcpListener("ep-dash-src", "Dashboard event listener", 2595, ["10.0.5.20"]),
    // Destinations
    tcpClient("ep-lis-out", "LIS HL7 out", "lis.lab.local", 2575),
    tcpClient("ep-lis-backup", "LIS HL7 out (backup)", "10.0.4.21", 2575),
    folder(
      "ep-archive",
      "Results archive folder",
      "D:\\LabData\\Results\\Archive",
      "results-*.csv",
    ),
    serial("ep-au680-orders", "AU680 order port", "COM30"),
    tcpClient("ep-xn-orders", "XN-1000 order link", "10.0.4.31", 4005),
    serial("ep-cobas-orders", "cobas order port", "COM31"),
    tcpClient("ep-lis-orders-in", "LIS orders in", "lis.lab.local", 2576),
    sftp(
      "ep-billing-sftp",
      "Billing export (SFTP)",
      "sftp.billing.example",
      "labbilling",
      "billing-sftp-password",
      "/inbound/lab",
    ),
    folder(
      "ep-worklist",
      "Analyzer worklist folder",
      "\\\\labfs01\\worklists\\out",
      "worklist-*.csv",
    ),
    tcpClient("ep-outreach-out", "Outreach HL7 out", "10.20.1.15", 6661),
    tcpClient("ep-dashboard", "Lab dashboard feed", "10.0.5.20", 8081),
  ];
}
