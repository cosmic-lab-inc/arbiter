## Build Dependencies

On Apple Silicon you need to install a custom C++ compiler since (at least for me) XCode's default compiler
was not working. You can install it with:

```shell
brew install gcc
```

The project uses a build script to compile so that it can link to the homebrew gcc compiler using the `CC`
environment variable.

## C++ Compile Bug

Somehow my rustc and XCode stopped communicating properly, so I had to manually target the C++ compiler.
You can override one of two ways:

#### 1) XCode Manual Flags

XCode installs a C++ here: `/Library/Developer/CommandLineTools/usr/bin/c++` but targeting that binary to compile a
basic C++ project doesn't work. CLion naturally adds flags to make it work, which also works if I manually add them:

```shell
CXXFLAGS="-g -arch arm64 -isysroot /Library/Developer/CommandLineTools/SDKs/MacOSX14.4.sdk"
```

#### 2) Homebrew

```shell
CXX=/opt/homebrew/bin/c++-14 cargo ...
```

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

## Backtesting

Download BTCUSD and ETHUSD 1 minute data from
Kaggle [here](https://www.kaggle.com/datasets/kaanxtr/btc-price-1m?resource=download).
Store the BTC CSV under `data/btc_1m.csv` and the ETH CSV under `data/eth_1m.csv`.