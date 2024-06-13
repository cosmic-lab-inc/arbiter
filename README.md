## Build Dependencies

You may need to install C++ dependencies to compile `protobuf-src`.
See this [readme](https://github.com/protocolbuffers/protobuf/blob/main/src/README.md).

## Development

Create a `.env` file and fill with an HTTP RPC url and a WSS url

```.dotenv
API_KEY=helius api key
WSS=wss://atlas-mainnet.helius-rpc.com?api-key=<API_KEY>
```

Run arbiter to test the transaction subscription with Helius:

```bash
cargo run -p arbiter
```
