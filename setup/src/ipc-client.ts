import * as net from "net";

// wire protocol type bytes (must match core/src/protocol.rs)
const MSG_SETUP_QUERY = 0x0b;
const MSG_SETUP_INFO = 0x0c;
const MSG_SETUP_TEST_SWITCH = 0x0d;
const MSG_SETUP_TEST_SWITCH_ACK = 0x0e;

const CONNECT_TIMEOUT_MS = 5000;
const READ_TIMEOUT_MS = 5000;

export interface MonitorInfo {
  id: string;
  name: string;
  manufacturer: string;
  model: string;
  serial: string;
  current_input_vcp: number | null;
  ddc_supported: boolean;
}

export interface SetupMonitorMapping {
  monitor_id: string;
  inputs: Record<string, string>;
}

export interface ServerSetupInfo {
  server_name: string;
  os: string;
  monitors: MonitorInfo[];
  monitor_inputs: SetupMonitorMapping[];
}

// encode a wire protocol frame: [4-byte BE length][1-byte type][JSON payload]
function encodeFrame(typeByte: number, payload: unknown): Buffer {
  const jsonStr = JSON.stringify(payload);
  const jsonBuf = Buffer.from(jsonStr, "utf-8");
  const payloadLen = 1 + jsonBuf.length;
  const frame = Buffer.alloc(4 + payloadLen);
  frame.writeUInt32BE(payloadLen, 0);
  frame[4] = typeByte;
  jsonBuf.copy(frame, 5);
  return frame;
}

// read exactly `n` bytes from a socket with timeout
function readExact(
  socket: net.Socket,
  n: number,
  timeoutMs: number,
): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    let buf = Buffer.alloc(0);
    const timer = setTimeout(() => {
      socket.destroy();
      reject(new Error("read timeout"));
    }, timeoutMs);

    const onData = (chunk: Buffer) => {
      buf = Buffer.concat([buf, chunk]);
      if (buf.length >= n) {
        clearTimeout(timer);
        socket.removeListener("data", onData);
        socket.removeListener("error", onError);
        socket.removeListener("close", onClose);
        // if we read more than needed, unshift the rest
        if (buf.length > n) {
          socket.unshift(buf.subarray(n));
        }
        resolve(buf.subarray(0, n));
      }
    };

    const onError = (err: Error) => {
      clearTimeout(timer);
      reject(err);
    };

    const onClose = () => {
      clearTimeout(timer);
      reject(new Error("connection closed"));
    };

    socket.on("data", onData);
    socket.on("error", onError);
    socket.on("close", onClose);
  });
}

// read one wire protocol frame and return { typeByte, json }
async function readFrame(
  socket: net.Socket,
): Promise<{ typeByte: number; json: unknown }> {
  const lenBuf = await readExact(socket, 4, READ_TIMEOUT_MS);
  const payloadLen = lenBuf.readUInt32BE(0);
  if (payloadLen === 0 || payloadLen > 16 * 1024 * 1024) {
    throw new Error(`invalid frame length: ${payloadLen}`);
  }
  const payload = await readExact(socket, payloadLen, READ_TIMEOUT_MS);
  const typeByte = payload[0];
  const jsonStr = payload.subarray(1).toString("utf-8");
  return { typeByte, json: JSON.parse(jsonStr) };
}

// connect to the orchestrator's agent listener port
function connectTcp(ip: string, port: number): Promise<net.Socket> {
  return new Promise((resolve, reject) => {
    const socket = net.createConnection({ host: ip, port }, () => {
      resolve(socket);
    });
    socket.setTimeout(CONNECT_TIMEOUT_MS);
    socket.on("timeout", () => {
      socket.destroy();
      reject(new Error("connection timeout"));
    });
    socket.on("error", (err: Error) => {
      reject(err);
    });
  });
}

// query the orchestrator for setup info
// connects to the agent listener port, sends SetupQuery, receives SetupInfo
export async function queryServerSetupInfo(
  ip: string,
  port: number,
): Promise<ServerSetupInfo | null> {
  let socket: net.Socket | null = null;
  try {
    socket = await connectTcp(ip, port);
    socket.pause(); // use manual reads

    // send SetupQuery: the serde-tagged JSON is {"SetupQuery":null}
    const frame = encodeFrame(MSG_SETUP_QUERY, { SetupQuery: null });
    socket.write(frame);

    // read SetupInfo response
    const { typeByte, json } = await readFrame(socket);
    if (typeByte !== MSG_SETUP_INFO) {
      return null;
    }

    // serde externally-tagged: { "SetupInfo": { ... } }
    const msg = json as Record<string, unknown>;
    const info = msg["SetupInfo"] as Record<string, unknown> | undefined;
    if (!info) return null;

    return {
      server_name: info["server_name"] as string,
      os: info["os"] as string,
      monitors: info["monitors"] as MonitorInfo[],
      monitor_inputs: info["monitor_inputs"] as SetupMonitorMapping[],
    };
  } catch {
    return null;
  } finally {
    socket?.destroy();
  }
}

// ask the orchestrator to switch a monitor to a specific input
// returns true if the switch succeeded
export async function requestTestSwitch(
  ip: string,
  port: number,
  monitorId: string,
  inputVcp: number,
): Promise<boolean> {
  let socket: net.Socket | null = null;
  try {
    socket = await connectTcp(ip, port);
    socket.pause();

    // send SetupQuery first to identify as setup session
    const queryFrame = encodeFrame(MSG_SETUP_QUERY, { SetupQuery: null });
    socket.write(queryFrame);

    // read and discard SetupInfo response
    await readFrame(socket);

    // send SetupTestSwitch
    const switchFrame = encodeFrame(MSG_SETUP_TEST_SWITCH, {
      SetupTestSwitch: {
        monitor_id: monitorId,
        input_vcp: inputVcp,
      },
    });
    socket.write(switchFrame);

    // read SetupTestSwitchAck
    const { typeByte, json } = await readFrame(socket);
    if (typeByte !== MSG_SETUP_TEST_SWITCH_ACK) {
      return false;
    }

    const msg = json as Record<string, unknown>;
    const ack = msg["SetupTestSwitchAck"] as Record<string, unknown> | undefined;
    return ack?.["success"] === true;
  } catch {
    return false;
  } finally {
    socket?.destroy();
  }
}
