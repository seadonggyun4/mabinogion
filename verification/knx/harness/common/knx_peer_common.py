#!/usr/bin/env python3
"""Shared KNXnet/IP smoke helpers for repo-owned KNX interop peers."""

from __future__ import annotations

import json
import socket
import time
from pathlib import Path
from typing import Any


class PeerTranscript:
    def __init__(self, target: str, peer: str, host: str, port: int) -> None:
        self.data: dict[str, Any] = {
            "schema_version": 1,
            "target": target,
            "peer": peer,
            "sut_addr": f"{host}:{port}",
            "capabilities": [],
            "steps": [],
            "failure_category": None,
            "errors": [],
            "artifacts": {},
        }

    def artifact(self, key: str, value: Any) -> None:
        self.data["artifacts"][key] = value

    def step(self, name: str, status: str, details: str = "") -> None:
        self.data["steps"].append(
            {
                "name": name,
                "status": status,
                "details": details,
            }
        )

    def capability(self, capability_id: str, status: str, details: str = "") -> None:
        self.data["capabilities"].append(
            {
                "id": capability_id,
                "status": status,
                "details": details,
            }
        )

    def fail(self, category: str, message: str) -> None:
        self.data["failure_category"] = category
        self.data["errors"].append(message)

    def write(self, path: str | Path) -> None:
        transcript_path = Path(path)
        transcript_path.parent.mkdir(parents=True, exist_ok=True)
        transcript_path.write_text(
            json.dumps(self.data, indent=2, sort_keys=True),
            encoding="utf-8",
        )

    def ok(self) -> bool:
        return self.data["failure_category"] is None and not self.data["errors"]


def empty_knx_frame(service_type: int) -> bytes:
    return bytes([0x06, 0x10, service_type >> 8, service_type & 0xFF, 0x00, 0x06])


def knx_frame(service_type: int, body: bytes) -> bytes:
    total = len(body) + 6
    return (
        bytes([0x06, 0x10, service_type >> 8, service_type & 0xFF])
        + total.to_bytes(2, "big")
        + body
    )


def hpai(port: int) -> bytes:
    return bytes([0x08, 0x01, 127, 0, 0, 1]) + port.to_bytes(2, "big")


def group_address_raw(address: str) -> int:
    main, middle, sub = [int(part) for part in address.split("/")]
    return (main << 11) | (middle << 8) | sub


def udp_socket(timeout_seconds: float = 3.0) -> socket.socket:
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.settimeout(timeout_seconds)
    sock.bind(("127.0.0.1", 0))
    return sock


def expect_service(frame: bytes, service_type: int, context: str) -> None:
    if len(frame) < 6:
        raise RuntimeError(f"{context}: short KNXnet/IP frame: {frame.hex()}")
    actual = int.from_bytes(frame[2:4], "big")
    if actual != service_type:
        raise RuntimeError(
            f"{context}: expected service 0x{service_type:04x}, got 0x{actual:04x}: {frame.hex()}"
        )


def discover(host: str, port: int) -> bytes:
    with udp_socket() as sock:
        sock.sendto(empty_knx_frame(0x0201), (host, port))
        response, _ = sock.recvfrom(2048)
    expect_service(response, 0x0202, "search")
    return response


def describe(host: str, port: int) -> bytes:
    with udp_socket() as sock:
        sock.sendto(empty_knx_frame(0x0203), (host, port))
        response, _ = sock.recvfrom(2048)
    expect_service(response, 0x0204, "description")
    return response


def raw_connect(sock: socket.socket, host: str, port: int) -> tuple[int, bytes]:
    local_port = sock.getsockname()[1]
    endpoint = hpai(local_port)
    connect_body = endpoint + endpoint + bytes([0x04, 0x04, 0x02, 0x00])
    sock.sendto(knx_frame(0x0205, connect_body), (host, port))
    connect_response, _ = sock.recvfrom(2048)
    expect_service(connect_response, 0x0206, "connect")
    if len(connect_response) < 8 or connect_response[7] != 0:
        raise RuntimeError(f"connect failed: {connect_response.hex()}")
    return connect_response[6], endpoint


def raw_connection_state(
    sock: socket.socket,
    host: str,
    port: int,
    channel_id: int,
    endpoint: bytes,
) -> int:
    state_body = bytes([channel_id, 0x00]) + endpoint
    sock.sendto(knx_frame(0x0207, state_body), (host, port))
    response, _ = sock.recvfrom(2048)
    expect_service(response, 0x0208, "connection state")
    if len(response) < 8:
        raise RuntimeError(f"connection state response too short: {response.hex()}")
    return response[7]


def raw_disconnect(
    sock: socket.socket,
    host: str,
    port: int,
    channel_id: int,
    endpoint: bytes,
) -> int:
    disconnect_body = bytes([channel_id, 0x00]) + endpoint
    sock.sendto(knx_frame(0x0209, disconnect_body), (host, port))
    response, _ = sock.recvfrom(2048)
    expect_service(response, 0x020A, "disconnect")
    if len(response) < 8:
        raise RuntimeError(f"disconnect response too short: {response.hex()}")
    return response[7]


def direct_tunnel_state(host: str, port: int) -> dict[str, Any]:
    with udp_socket() as sock:
        channel_id, endpoint = raw_connect(sock, host, port)
        state_status = raw_connection_state(sock, host, port, channel_id, endpoint)
        disconnect_status = raw_disconnect(sock, host, port, channel_id, endpoint)
    return {
        "channel_id": channel_id,
        "state_status": state_status,
        "disconnect_status": disconnect_status,
    }


def raw_write_group_value(host: str, port: int, group_address: str, write_value: int) -> None:
    if write_value < 0 or write_value > 0x3F:
        raise RuntimeError("raw writer expects a compact 6-bit value")

    with udp_socket() as sock:
        channel_id, endpoint = raw_connect(sock, host, port)
        destination = group_address_raw(group_address)
        cemi = (
            bytes([0x11, 0x00, 0xAC, 0x86])
            + bytes([0x11, 0x0A])
            + destination.to_bytes(2, "big")
            + bytes([0x01, 0x80 | (write_value & 0x3F)])
        )
        request_body = bytes([0x04, channel_id, 0x00, 0x00]) + cemi
        sock.sendto(knx_frame(0x0420, request_body), (host, port))
        ack, _ = sock.recvfrom(2048)
        expect_service(ack, 0x0421, "group write ack")
        if ack[7:10] != bytes([channel_id, 0x00, 0x00]):
            raise RuntimeError(f"group write was not acknowledged: {ack.hex()}")
        raw_disconnect(sock, host, port, channel_id, endpoint)


def raw_read_group_value(host: str, port: int, group_address: str) -> int:
    with udp_socket() as sock:
        channel_id, endpoint = raw_connect(sock, host, port)

        destination = group_address_raw(group_address)
        cemi = (
            bytes([0x11, 0x00, 0xAC, 0x86])
            + bytes([0x11, 0x0A])
            + destination.to_bytes(2, "big")
            + bytes([0x01, 0x00])
        )
        request_body = bytes([0x04, channel_id, 0x00, 0x00]) + cemi
        sock.sendto(knx_frame(0x0420, request_body), (host, port))

        deadline = time.monotonic() + 3.0
        while time.monotonic() < deadline:
            response, addr = sock.recvfrom(2048)
            service = int.from_bytes(response[2:4], "big")
            body = response[6:]
            if service == 0x0421:
                continue
            if service != 0x0420 or len(body) < 14:
                continue

            response_channel = body[1]
            response_sequence = body[2]
            ack_body = bytes([0x04, response_channel, response_sequence, 0x00])
            sock.sendto(knx_frame(0x0421, ack_body), addr)

            cemi = body[4:]
            add_info_len = cemi[1]
            offset = 2 + add_info_len + 2 + 2 + 2 + 1
            if len(cemi) <= offset:
                continue
            apci = cemi[offset]
            if (apci & 0xC0) == 0x40:
                raw_disconnect(sock, host, port, channel_id, endpoint)
                return apci & 0x3F

        raw_disconnect(sock, host, port, channel_id, endpoint)
        raise RuntimeError("group read timed out waiting for GroupValueResponse")


def group_round_trip(host: str, port: int, group_address: str, write_value: int) -> int:
    raw_write_group_value(host, port, group_address, write_value)
    time.sleep(0.2)
    return raw_read_group_value(host, port, group_address)
