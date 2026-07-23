import asyncio

import qrpc


async def main():
    response = await qrpc.unary_roundtrip(b"hello")
    print(response.decode("utf-8"))


if __name__ == "__main__":
    asyncio.run(main())
