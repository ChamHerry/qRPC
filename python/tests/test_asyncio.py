import asyncio

import qrpc


def test_unary_roundtrip():
    assert asyncio.run(qrpc.unary_roundtrip(b"ping")) == b"echo:ping"


def test_concurrent_unary_roundtrips_use_asyncio_bridge():
    async def run():
        payloads = [f"p{i}".encode() for i in range(4)]
        return await asyncio.gather(*(qrpc.unary_roundtrip(item) for item in payloads))

    assert asyncio.run(run()) == [b"echo:p0", b"echo:p1", b"echo:p2", b"echo:p3"]


def test_stream_roundtrip_preserves_order():
    assert asyncio.run(qrpc.stream_roundtrip([b"a", b"b", b"c"])) == [
        b"echo:a",
        b"echo:b",
        b"echo:c",
    ]
