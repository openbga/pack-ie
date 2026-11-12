# Configure a first channel

Start the engine using [Install](install.md), connect the console, and set
`PACKIE_BOOTSTRAP` as described there before using the CLI. Use synthetic results for the
first exchange and check the received values in the LIS before using a live interface.

These are two independent, complete **runtime** documents for an otherwise empty engine.
Save each code block with the filename shown. Applying a document replaces the whole
runtime configuration: to add a channel to an existing installation, first export it with
`packie config show`, then add the new endpoints and channel to that document. Keep its
`config_version` so concurrent changes are detected. Do not overwrite a running engine's
`runtime.json` directly.

## 1. Serial analyzer to a TCP listener

The analyzer uses serial; the LIS connects to PACK-IE over TCP and speaks the analyzer's
protocol itself. PACK-IE passes both directions unchanged. This example observes ASTM framing
to count messages; it does not acknowledge the analyzer or transform results.

Replace these installation values before applying:

| Setting | Example | Set it to |
| --- | --- | --- |
| Serial port | `COM3` | The engine computer's port: for example `/dev/ttyUSB0` on Linux or `/dev/tty.usbserial-0001` on macOS. |
| Serial parameters | 9600, 8 data bits, no parity, one stop bit, no flow control | The analyzer's interface settings, including hardware flow control if required. |
| Listener | `192.168.10.10:5100` | An address assigned to the engine computer and an unused port. |
| Allowed peer | `192.168.10.20/32` | The LIS computer's address. Configure the LIS to connect to the listener above. |

Save as `bridge.json`:

```json
{
  "schema_version": 1,
  "endpoints": [
    {
      "endpoint_id": "analyzer-serial",
      "label": "Analyzer serial port",
      "kind": "serial",
      "settings": {
        "port_name": "COM3",
        "baud_rate": 9600,
        "data_bits": 8,
        "parity": "none",
        "stop_bits": "one",
        "flow_control": "none"
      }
    },
    {
      "endpoint_id": "lis-bridge",
      "label": "LIS bridge listener",
      "kind": "tcp_listener",
      "settings": {
        "bind_address": "192.168.10.10",
        "port": 5100,
        "allowed_peer_cidrs": ["192.168.10.20/32"]
      }
    }
  ],
  "channels": [
    {
      "channel_id": "analyzer-bridge",
      "label": "Analyzer bridge to LIS",
      "kind": "bridge",
      "endpoint_a": "analyzer-serial",
      "endpoint_b": "lis-bridge",
      "connection": {
        "connect_strategy": "receiver_then_sender",
        "receiver": "b",
        "pairing_timeout_ms": 5000
      },
      "tap": {
        "framer": "astm",
        "record_messages": false
      }
    }
  ]
}
```

Receiver B is prepared first. Until the LIS connects, the listener waits and the serial
side is disconnected. After that connection, PACK-IE opens the serial port and pairs the two
sides. Closing either side closes the other and starts recovery. Buffers are bounded per
direction (defaults: 1000 segments and 10 MiB); a slow receiver holds the sender back.

### Create it in the console

1. In **Endpoints**, create a serial endpoint with the id, label and serial settings above.
   Choose **Validate**, then **Save and apply**, review the plan and confirm.
2. Create the TCP listener endpoint with its bind address, port and allowed peer. Validate
   and apply it too. An endpoint with no running channel is disconnected; that is expected.
3. In **Channels**, create a bridge with id `analyzer-bridge`. Set Endpoint A to the serial
   endpoint and Endpoint B to the listener. Choose **Receiver first, then sender** and
   **Receiver: Endpoint B**. Set **Tap framer: ASTM**. Keep the buffer and reconnect defaults.
4. Validate, inspect the plan, then save and apply. Check the per-channel outcome and active
   version. Validation errors must be fixed before apply; review warnings too.

### Or apply the file from the CLI

Run from the directory containing `bridge.json`:

```sh
packie config validate bridge.json
packie config apply bridge.json --note "First serial bridge"
packie status
```

`validate` works without a running engine and checks the document, including mapping paths;
it does not open ports or prove that a network address exists. `apply` needs the engine.

### Check the exchange

On the Dashboard, the channel starts in **Starting**, with the listener **Listening** and
session **Waiting for peer**. Once the LIS connects and the serial port opens, the channel
is **Running**, both sides are connected and session is **Bridged**. Send a synthetic analyzer
result: bytes should rise in A→B, with replies in B→A. Open the channel to check ASTM message
counts, last activity and low buffer fill. A tap counts complete observed transfers, not
clinical acceptance. With `record_messages: false`, no tap bodies appear in Messages. The
live transcript works independently of this setting.

## 2. ASTM results to HL7 ORU over MLLP

Here PACK-IE receives ASTM over TCP, stores it, then maps it to HL7 `ORU^R01` and connects to the
LIS's MLLP listener. An analyzer frame ACK means PACK-IE has stored that frame; it does not mean
the LIS accepted the result.

Use an analyzer format with one patient and one specimen per ASTM message, numeric results,
`P.3` as patient id, `O.3` as specimen id, and `R.3.4` as the test code. This small example
assumes final results and writes `F` to OBX-11. Its local test codes, units, identifiers and
result status must match the LIS interface agreement. It does not group multiple patients
or specimens into separate orders. Extend the mapping for the actual analyzer format.

Set `192.168.10.10` to the engine's address, `192.168.10.30/32` to the analyzer's address, and
`192.168.10.20:6661` to the LIS MLLP listener. Configure the analyzer to connect to the engine
on port 5200. Save as `astm-to-hl7.json`:

```json
{
  "schema_version": 1,
  "endpoints": [
    {
      "endpoint_id": "analyzer-astm",
      "label": "Analyzer ASTM listener",
      "kind": "tcp_listener",
      "settings": {
        "bind_address": "192.168.10.10",
        "port": 5200,
        "allowed_peer_cidrs": ["192.168.10.30/32"]
      }
    },
    {
      "endpoint_id": "lis-hl7",
      "label": "LIS MLLP destination",
      "kind": "tcp_client",
      "settings": { "host": "192.168.10.20", "port": 6661 }
    }
  ],
  "channels": [
    {
      "channel_id": "analyzer-results",
      "label": "Analyzer results to LIS",
      "kind": "message",
      "source": {
        "endpoint": "analyzer-astm",
        "protocol": "astm",
        "settings": { "role": "computer", "charset": "latin1" },
        "duplicate_detection": true
      },
      "destinations": [
        {
          "destination_id": "d1",
          "label": "LIS",
          "endpoint": "lis-hl7",
          "protocol": "hl7_mllp",
          "settings": { "ack_mode": "original" },
          "mapping": {
            "tables": {
              "tests": {
                "GLU": "GLU^Glucose^L",
                "NA": "NA^Sodium^L"
              }
            },
            "rules": [
              { "op": "constant", "value": "ORU^R01", "to": "MSH.9" },
              { "op": "constant", "value": "PACKIE", "to": "MSH.3" },
              { "op": "constant", "value": "LIS", "to": "MSH.5" },
              { "op": "copy", "from": "P.3", "to": "PID.3", "required": true },
              { "op": "copy", "from": "O.3", "to": "OBR.3", "required": true },
              { "op": "for_each", "over": "R", "into": "OBX", "rules": [
                { "op": "sequence", "to": "$.1" },
                { "op": "constant", "value": "NM", "to": "$.2" },
                { "op": "lookup", "from": "$.3.4", "table": "tests", "to": "$.3", "on_missing": "error" },
                { "op": "copy", "from": "$.4", "to": "$.5", "required": true },
                { "op": "copy", "from": "$.5", "to": "$.6" },
                { "op": "constant", "value": "F", "to": "$.11" }
              ] }
            ]
          },
          "delivery": {
            "max_attempts": 5,
            "ack_required": "accepted",
            "ack_timeout_seconds": 30,
            "retry_unacknowledged": false
          }
        }
      ],
      "retention": { "max_age_days": 90, "max_messages": null }
    }
  ]
}
```

The loop creates an OBX for every result record. Inside it, a `from` path beginning `$.`
reads the current ASTM R record; a `to` path beginning `$.` writes the new HL7 OBX. The
`tests` table maps analyzer codes to local LIS codes. A missing code fails the mapping
instead of silently using an unknown code. PACK-IE generates the HL7 time, control id, processing
id (`P`) and version (`2.5.1`); the mapping supplies the message type and application names.

### Create it in the console

1. Create and apply the two endpoints above: TCP listener for the analyzer, TCP client for
   the LIS. Use the same endpoint ids and network settings.
2. Create a **Message** channel with id `analyzer-results`. Select the analyzer endpoint as
   source, **ASTM**, **Link role: Computer**, and leave duplicate detection enabled.
3. Add one destination labelled **LIS**, select `lis-hl7`, **HL7 MLLP**, and
   **Acknowledgement mode: Original**. The console assigns its destination id.
4. Add a destination code table named `tests`; add the two code/text rows shown above.
   Add mapping rules in the JSON order. Select each `Rule` type, then enter its **From**,
   **To**, **Value**, **Table** and **Required** fields as applicable. For the loop choose
   `for_each`, **Over: R**, **Into: OBX**, and add its six rules inside it. For the lookup,
   **On missing: Fail the message** corresponds to `on_missing: error`.
5. Leave filters empty, keep maximum attempts at 5 and retention at 90 days. Validate,
   review the plan and save and apply. The advanced delivery fields shown in the JSON use
   the engine defaults; they do not need separate entries in the editor.

### Or apply the file from the CLI

```sh
packie config validate astm-to-hl7.json
packie config apply astm-to-hl7.json --note "First ASTM to HL7 channel"
packie status
packie messages list --channel analyzer-results --since 1h
```

These examples omit `config_version` for first setup; apply uses the active version. For
later edits, start with `packie config show` and retain its version. A conflict means reload
and review the intervening change. A validation error leaves the previous configuration
active. A **partial** apply saves the new version but reports which channel changes failed;
check those outcomes rather than relying on the version number alone.

### Check receipt and delivery

With the source bound and LIS connected, the Dashboard should show **Running**, source
**Listening**, and LIS **Connected**. An idle listening analyzer source is ready, even with
no analyzer attached. After a synthetic result arrives, received and acknowledged counts
rise, last activity updates and the queue returns to zero. There should be no unresolved
delivery or active alert.

In **Messages**, filter to `analyzer-results` and open the result. The message is `routed`;
the LIS delivery should be `acknowledged`, level `accepted`, code `AA`. Compare original ASTM
and transformed HL7, including all result records and units. An LIS `AA` confirms its
application accepted the message; it does not prove that a person reviewed the result.
If the destination is down, PACK-IE can keep receiving and queuing results. See
[Running and monitoring](running-and-monitoring.md) for states and actions, or
[Troubleshooting](troubleshooting.md) for recovery.
