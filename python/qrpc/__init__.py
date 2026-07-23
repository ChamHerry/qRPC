"""qRPC Python asyncio binding."""

from __future__ import annotations

import asyncio
import threading
from collections.abc import Iterable

from . import _qrpc


async def _call_threadsafe(func, *args):
    loop = asyncio.get_running_loop()
    future = loop.create_future()

    def worker():
        try:
            result = func(*args)
        except BaseException as exc:  # pragma: no cover - exercised on failures
            loop.call_soon_threadsafe(future.set_exception, exc)
        else:
            loop.call_soon_threadsafe(future.set_result, result)

    threading.Thread(target=worker, daemon=True).start()
    return await future


class Client:
    async def unary(self, method: str, request: bytes) -> bytes:
        if not method:
            raise ValueError("method is required")
        return await _call_threadsafe(_qrpc.unary_roundtrip, bytes(request))

    async def stream(self, method: str, chunks: Iterable[bytes]) -> list[bytes]:
        responses = []
        for chunk in chunks:
            responses.append(await self.unary(method, bytes(chunk)))
        return responses


async def unary_roundtrip(payload: bytes) -> bytes:
    return await Client().unary("/demo.Echo/Unary", payload)


async def stream_roundtrip(chunks: Iterable[bytes]) -> list[bytes]:
    return await Client().stream("/demo.Echo/Stream", chunks)


__all__ = ["Client", "stream_roundtrip", "unary_roundtrip"]
