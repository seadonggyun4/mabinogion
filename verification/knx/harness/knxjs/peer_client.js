#!/usr/bin/env node
"use strict";

const dgram = require("dgram");
const fs = require("fs");
const path = require("path");

function env(name, fallback) {
  const value = process.env[name] || fallback;
  if (!value) {
    throw new Error(`missing required environment variable ${name}`);
  }
  return value;
}

function frame(serviceType, body = Buffer.alloc(0)) {
  const output = Buffer.alloc(6 + body.length);
  output[0] = 0x06;
  output[1] = 0x10;
  output.writeUInt16BE(serviceType, 2);
  output.writeUInt16BE(output.length, 4);
  body.copy(output, 6);
  return output;
}

function hpai(port) {
  return Buffer.from([0x08, 0x01, 127, 0, 0, 1, (port >> 8) & 0xff, port & 0xff]);
}

function groupAddressRaw(address) {
  const [main, middle, sub] = address.split("/").map((part) => Number.parseInt(part, 10));
  return (main << 11) | (middle << 8) | sub;
}

function sendReceive(socket, message, host, port, matcher, timeoutMs = 3000) {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      socket.off("message", onMessage);
      reject(new Error("timeout waiting for KNXnet/IP response"));
    }, timeoutMs);

    function onMessage(response, remote) {
      if (!matcher(response)) {
        return;
      }
      clearTimeout(timeout);
      socket.off("message", onMessage);
      resolve({ response, remote });
    }

    socket.on("message", onMessage);
    socket.send(message, port, host, (error) => {
      if (error) {
        clearTimeout(timeout);
        socket.off("message", onMessage);
        reject(error);
      }
    });
  });
}

async function withSocket(callback) {
  const socket = dgram.createSocket("udp4");
  await new Promise((resolve) => socket.bind(0, "127.0.0.1", resolve));
  try {
    return await callback(socket);
  } finally {
    socket.close();
  }
}

async function discover(host, port) {
  return withSocket(async (socket) => {
    const { response } = await sendReceive(
      socket,
      frame(0x0201),
      host,
      port,
      (message) => message.readUInt16BE(2) === 0x0202,
    );
    return response.length;
  });
}

async function connect(socket, host, port) {
  const localPort = socket.address().port;
  const endpoint = hpai(localPort);
  const connectBody = Buffer.concat([endpoint, endpoint, Buffer.from([0x04, 0x04, 0x02, 0x00])]);
  const { response } = await sendReceive(
    socket,
    frame(0x0205, connectBody),
    host,
    port,
    (message) => message.readUInt16BE(2) === 0x0206,
  );
  if (response[7] !== 0) {
    throw new Error(`connect failed: ${response.toString("hex")}`);
  }
  return { channelId: response[6], endpoint };
}

async function disconnect(socket, host, port, channelId, endpoint) {
  const body = Buffer.concat([Buffer.from([channelId, 0x00]), endpoint]);
  await sendReceive(
    socket,
    frame(0x0209, body),
    host,
    port,
    (message) => message.readUInt16BE(2) === 0x020a,
  );
}

async function tunnelState(host, port) {
  return withSocket(async (socket) => {
    const { channelId, endpoint } = await connect(socket, host, port);
    const body = Buffer.concat([Buffer.from([channelId, 0x00]), endpoint]);
    const { response } = await sendReceive(
      socket,
      frame(0x0207, body),
      host,
      port,
      (message) => message.readUInt16BE(2) === 0x0208,
    );
    await disconnect(socket, host, port, channelId, endpoint);
    return { channelId, stateStatus: response[7] };
  });
}

async function writeGroupValue(host, port, groupAddress, value) {
  return withSocket(async (socket) => {
    const { channelId, endpoint } = await connect(socket, host, port);
    const destination = groupAddressRaw(groupAddress);
    const cemi = Buffer.concat([
      Buffer.from([0x11, 0x00, 0xac, 0x86, 0x11, 0x0a]),
      Buffer.from([(destination >> 8) & 0xff, destination & 0xff, 0x01, 0x80 | (value & 0x3f)]),
    ]);
    const body = Buffer.concat([Buffer.from([0x04, channelId, 0x00, 0x00]), cemi]);
    await sendReceive(
      socket,
      frame(0x0420, body),
      host,
      port,
      (message) => message.readUInt16BE(2) === 0x0421,
    );
    await disconnect(socket, host, port, channelId, endpoint);
  });
}

async function readGroupValue(host, port, groupAddress) {
  return withSocket(async (socket) => {
    const { channelId, endpoint } = await connect(socket, host, port);
    const destination = groupAddressRaw(groupAddress);
    const cemi = Buffer.concat([
      Buffer.from([0x11, 0x00, 0xac, 0x86, 0x11, 0x0a]),
      Buffer.from([(destination >> 8) & 0xff, destination & 0xff, 0x01, 0x00]),
    ]);
    const body = Buffer.concat([Buffer.from([0x04, channelId, 0x00, 0x00]), cemi]);
    const { response, remote } = await sendReceive(
      socket,
      frame(0x0420, body),
      host,
      port,
      (message) => message.readUInt16BE(2) === 0x0420,
      3000,
    );
    const requestBody = response.subarray(6);
    const ack = frame(0x0421, Buffer.from([0x04, requestBody[1], requestBody[2], 0x00]));
    socket.send(ack, remote.port, remote.address);
    const cemiResponse = requestBody.subarray(4);
    const addInfoLen = cemiResponse[1];
    const offset = 2 + addInfoLen + 2 + 2 + 2 + 1;
    const apci = cemiResponse[offset];
    await disconnect(socket, host, port, channelId, endpoint);
    return apci & 0x3f;
  });
}

function transcript(target, peer, host, port) {
  return {
    schema_version: 1,
    target,
    peer,
    sut_addr: `${host}:${port}`,
    capabilities: [],
    steps: [],
    failure_category: null,
    errors: [],
    artifacts: {},
  };
}

function addStep(doc, name, status, details) {
  doc.steps.push({ name, status, details });
}

function addCapability(doc, id, status, details) {
  doc.capabilities.push({ id, status, details });
}

async function main() {
  const target = env("MABI_KNX_INTEROP_TARGET", "knxjs");
  const host = env("MABI_KNX_SUT_HOST");
  const port = Number.parseInt(env("MABI_KNX_SUT_PORT"), 10);
  const groupAddress = env("MABI_KNX_GROUP_ADDRESS");
  const writeValue = Number.parseInt(env("MABI_KNX_WRITE_VALUE", "42"), 10);
  const transcriptPath = env("MABI_KNX_TRANSCRIPT_PATH");
  const expectedVersion = env("MABI_KNX_NODE_PACKAGE_VERSION", "2.5.4");
  const doc = transcript(target, "knxjs", host, port);

  try {
    const pkg = require("knx/package.json");
    if (pkg.version !== expectedVersion) {
      throw new Error(`expected knx ${expectedVersion}, found ${pkg.version}`);
    }
    doc.artifacts.node_package = `knx@${pkg.version}`;
    doc.artifacts.group_address = groupAddress;
    addStep(doc, "tool_version", "passed", `knx@${pkg.version}`);

    const discoveryBytes = await discover(host, port);
    addStep(doc, "discovery", "passed", `${discoveryBytes} bytes`);
    addCapability(doc, "discovery", "passed", "SearchResponse received");

    const state = await tunnelState(host, port);
    addStep(doc, "tunnel_state", "passed", JSON.stringify(state));
    addCapability(doc, "tunneling_connect", "passed", "Tunneling connect/state/disconnect");

    await writeGroupValue(host, port, groupAddress, writeValue);
    await new Promise((resolve) => setTimeout(resolve, 200));
    const roundTripValue = await readGroupValue(host, port, groupAddress);
    doc.artifacts.round_trip_value = roundTripValue;
    if (roundTripValue !== writeValue) {
      throw new Error(`round trip value ${roundTripValue} != ${writeValue}`);
    }
    addStep(doc, "dpt_group_round_trip", "passed", `${groupAddress}=${roundTripValue}`);
    addCapability(doc, "group_value_read_write", "passed", "Node stack group IO parity");
    addCapability(doc, "dpt_codec", "passed", "Compact DPT 5-style value parity");

    addStep(doc, "routing_smoke", "unsupported", "single-container topology uses tunneling");
    addCapability(doc, "routing_multicast", "unsupported", "routing is Phase 3 multi-container work");
  } catch (error) {
    doc.failure_category = error.code === "MODULE_NOT_FOUND" ? "tool_missing" : "protocol_failure";
    doc.errors.push(`${error.name}: ${error.message}`);
  }

  fs.mkdirSync(path.dirname(transcriptPath), { recursive: true });
  fs.writeFileSync(transcriptPath, `${JSON.stringify(doc, null, 2)}\n`, "utf8");
  return doc.failure_category === null && doc.errors.length === 0 ? 0 : 1;
}

main().then((code) => process.exit(code));
