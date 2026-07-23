#!/usr/bin/env python3
"""Python asyncio bridge pressure probe."""

from __future__ import annotations

import asyncio

import qrpc


async def main() -> None:
    payloads = [f"payload-{idx}".encode() * 512 for idx in range(32)]
    responses = await asyncio.gather(*(qrpc.unary_roundtrip(payload) for payload in payloads))
    for payload, response in zip(payloads, responses, strict=True):
        expected = b"echo:" + payload
        if response != expected:
            raise AssertionError(f"unexpected response length={len(response)}")
    print("python pressure probe PASS")


if __name__ == "__main__":
    asyncio.run(main())
