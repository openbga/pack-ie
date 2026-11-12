// Synthetic peers for the manual check (MANUAL.md). Needs Node 22 or newer; no packages.
//   node peers.mjs bridge   instrument client + system server around the bridge (ports 5301/5302)
//   node peers.mjs feed     sends one synthetic HL7 ORU every 2 seconds to 5401, prints the ACKs;
//                           type `revise` + Enter in its terminal to send the revision pair below
//                           over the same connection (the HL7 listener takes one peer at a time)
//   node peers.mjs sink     an HL7 receiver on 5402 that answers AA (the LIS the channel delivers to)
//   node peers.mjs revise   sends HL7 result CTRL-REVISED-1 twice, the second time corrected (a revision),
//                           then exits; needs the listener free: stop `feed` first, or use its `revise` command
import net from "node:net";
import readline from "node:readline";

const mode = process.argv[2];
const VT = "\x0b";
const FS = "\x1c";
const CR = "\r";
const say = (...parts) => process.stdout.write(`${parts.join(" ")}\n`);
const stamp = () => new Date().toISOString().replace(/[-:T]/g, "").slice(0, 14);

/** HL7 result CTRL-REVISED-1 with `value` as its glucose: the same source identity every time. */
const revisedResult = (value) =>
  [
    `MSH|^~\\&|SIMLAB|SIMFAC|PACKIE|PACKIEFAC|${stamp()}||ORU^R01|CTRL-REVISED-1|P|2.5.1`,
    "PID|1||PID-MANUAL-2000||TEST^REVISED||19700101|F",
    "OBR|1||SID-REVISED-1|GLU^Glucose",
    `OBX|1|NM|GLU^Glucose||${value}|mg/dL|70-110|N|||F|||${stamp()}`,
  ].join(CR) + CR;

/** Sends the result with 140, then, 1.5 s later, the corrected one (104) on `socket`. */
function sendRevisionPair(socket, who) {
  say(`[${who}] sending CTRL-REVISED-1 with 140, then corrected to 104`);
  socket.write(VT + revisedResult(140) + FS + CR);
  setTimeout(() => socket.write(VT + revisedResult(104) + FS + CR), 1500);
}

function bridge() {
  net
    .createServer((socket) => {
      say("[system] the bridge connected");
      socket.on("data", (data) => {
        for (const line of data.toString().split("\n").filter(Boolean)) {
          socket.write(`pong ${line.split(" ")[1]}\n`);
        }
      });
      socket.on("error", () => {});
    })
    .listen(5302, "127.0.0.1", () => say("[system] listening on 5302"));
  const instrument = () => {
    const socket = net.connect(5301, "127.0.0.1");
    let timer;
    let n = 0;
    socket.on("connect", () => {
      say("[instrument] connected to the bridge");
      timer = setInterval(() => socket.write(`ping ${++n}\n`), 500);
    });
    socket.on("data", (data) => process.stdout.write(`[instrument] got ${data}`));
    socket.on("close", () => {
      clearInterval(timer);
      setTimeout(instrument, 1000);
    });
    socket.on("error", () => {});
  };
  setTimeout(instrument, 1500);
}

function feed() {
  let n = 0;
  let current = null;
  // `revise` typed in this terminal sends the revision pair over the feed's own connection.
  readline.createInterface({ input: process.stdin }).on("line", (line) => {
    if (line.trim() !== "revise") return;
    if (current === null) say("[feed] not connected yet; try again in a second");
    else sendRevisionPair(current, "feed");
  });
  const connect = () => {
    const socket = net.connect(5401, "127.0.0.1");
    let timer;
    socket.on("connect", () => {
      current = socket;
      say("[feed] connected");
      timer = setInterval(() => {
        n += 1;
        const message =
          [
            `MSH|^~\\&|SIMLAB|SIMFAC|PACKIE|PACKIEFAC|${stamp()}||ORU^R01|CTRL-MANUAL-${n}|P|2.5.1`,
            `PID|1||PID-MANUAL-${1000 + (n % 7)}||TEST^PATIENT${n % 7}||19700101|F`,
            `OBR|1||SID-MANUAL-${n}|GLU^Glucose`,
            `OBX|1|NM|GLU^Glucose||${80 + (n % 40)}|mg/dL|70-110|N|||F|||${stamp()}`,
          ].join(CR) + CR;
        socket.write(VT + message + FS + CR);
      }, 2000);
    });
    socket.on("data", (data) =>
      say(
        "[feed] ack:",
        data
          .toString("latin1")
          .replaceAll(VT, "")
          .replaceAll(FS, "")
          .split(CR)
          .filter(Boolean)
          .join(" / "),
      ),
    );
    socket.on("close", () => {
      clearInterval(timer);
      current = null;
      setTimeout(connect, 1000);
    });
    socket.on("error", () => {});
  };
  setTimeout(connect, 500);
}

function sink() {
  net
    .createServer((socket) => {
      let buffer = "";
      socket.on("data", (data) => {
        buffer += data.toString("latin1");
        while (buffer.includes(FS + CR)) {
          const end = buffer.indexOf(FS + CR);
          const message = buffer.slice(buffer.indexOf(VT) + 1, end);
          buffer = buffer.slice(end + 2);
          const control = message.split(CR)[0].split("|")[9];
          say(`[sink] received ${control}, answering AA`);
          socket.write(
            `${VT}MSH|^~\\&|SINK|LIS|PACKIE|LAB|${stamp()}||ACK|A${control}|P|2.5.1${CR}MSA|AA|${control}${CR}${FS}${CR}`,
          );
        }
      });
      socket.on("error", () => {});
    })
    .listen(5402, "127.0.0.1", () => say("[sink] listening on 5402"));
}

function revise() {
  const socket = net.connect(5401, "127.0.0.1");
  socket.on("connect", () => {
    sendRevisionPair(socket, "revise");
    setTimeout(() => socket.end(), 3000);
  });
  socket.on("data", (data) =>
    say(
      "[revise] ack:",
      data
        .toString("latin1")
        .replaceAll(VT, "")
        .replaceAll(FS, "")
        .split(CR)
        .filter(Boolean)
        .join(" / "),
    ),
  );
  socket.on("error", (error) =>
    say(
      "[revise]",
      error.message,
      "(the HL7 listener takes one peer at a time: stop `feed`, or type `revise` in its terminal)",
    ),
  );
}

const modes = { bridge, feed, sink, revise };
if (!modes[mode]) {
  say("usage: node peers.mjs bridge|feed|sink|revise");
  process.exit(2);
}
modes[mode]();
