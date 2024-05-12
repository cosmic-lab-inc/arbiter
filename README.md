Create a `.env` file and fill with an HTTP RPC url and a WSS url

```.dotenv
RPC_URL=https://mainnet.helius-rpc.com/?api-key=<API_KEY>
WSS=wss://atlas-mainnet.helius-rpc.com?api-key=<API_KEY>
```

Run arbiter to test the transaction subscription with Helius:

```bash
cargo run -p arbiter
```
