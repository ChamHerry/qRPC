#!/usr/bin/env python3
"""Python asyncio cancellation probe."""

from __future__ import annotations

import asyncio

import qrpc


async def main() -> None:
    task = asyncio.create_task(qrpc.unary_roundtrip(b"cancel-me"))
    task.cancel()
    try:
        await task
    except asyncio.CancelledError:
        pass
    else:  # pragma: no cover
        raise AssertionError("task cancellation did not propagate")

    try:
        await asyncio.wait_for(asyncio.sleep(0.05), timeout=0.001)
    except TimeoutError:
        pass
    else:  # pragma: no cover
        raise AssertionError("deadline timeout did not fire")

    print("python cancel/deadline probe PASS")


if __name__ == "__main__":
    asyncio.run(main())
