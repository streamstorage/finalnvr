## FinalNVR

FinalNVR is a ready-to-use NVR system that allows user to view, store and analyze video streams from IP cameras in real time.

FinalNVR's backend storage is [pravega](https://github.com/pravega/pravega) which is a decent streaming storage system for video streams.

### Installation

Run the following command to start.

```
$ yarn

# Build for production
$ yarn build

# Build for production and view the bundle analyzer report.
$ yarn build --report

$ cd src-backend && cargo build --release
```

### Development

```
# Install dev tools
$ cargo install cargo-watch
$ rustup component add clippy

# Serve frontend with hot reload at 0.0.0.0:5173 by default.
$ yarn dev --host

# Serve backend with hot reload at 0.0.0.0:8080
$ cd src-backend && RUST_LOG=debug cargo watch -w src -w Cargo.toml -x 'run'

# Linting
$ yarn lint
$ cd src-backend && cargo clippy --fix
```

### License

[MIT](https://github.com/epicmaxco/vuestic-admin/blob/master/LICENSE) license.
