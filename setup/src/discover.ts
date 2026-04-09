import { createSocket } from "dgram";

const DISCOVERY_PORT = 24802;
const DISCOVERY_MAGIC = "SOFTKVM_DISCOVER";
const DISCOVERY_TIMEOUT_MS = 2000;

export interface ServerInfo {
  name: string;
  version: string;
  ip: string;
  port: number;
  os: string;
}

// scan the local network for running softkvm servers
export async function discoverServers(): Promise<ServerInfo[]> {
  return new Promise((resolve) => {
    const servers: ServerInfo[] = [];
    const socket = createSocket("udp4");

    socket.on("message", (msg: Buffer) => {
      const text = msg.toString();
      const parts = text.split(":");
      if (parts.length === 6 && parts[0] === "SOFTKVM_HERE") {
        servers.push({
          name: parts[1],
          version: parts[2],
          ip: parts[3],
          port: parseInt(parts[4], 10),
          os: parts[5],
        });
      }
    });

    socket.bind(0, () => {
      socket.setBroadcast(true);
      const message = Buffer.from(DISCOVERY_MAGIC);
      socket.send(
        message,
        0,
        message.length,
        DISCOVERY_PORT,
        "255.255.255.255",
      );
    });

    setTimeout(() => {
      socket.close();
      resolve(servers);
    }, DISCOVERY_TIMEOUT_MS);
  });
}
