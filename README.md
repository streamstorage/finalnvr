## FinalNVR

FinalNVR is a ready-to-use NVR system that allows user to view, store and analyze video streams from IP cameras in real time.

FinalNVR's backend storage is [Pravega](https://github.com/pravega/pravega) which is a decent streaming storage system for video streams.

### Benefits of using Pravega vs. a NAS-only solution

1. Pravega writes data asynchronously to Long Term Storage (ECS, Isilon, or HDFS) in large blocks, which means that cost-effective magnetic media can be used. Without Pravega, all writes must go to Long Term Storage immediately, requiring either expensive low-latency hardware or reducing durability guarantees.

2. Video frames are written to Pravega atomically. Pravega guarantees that the stream will not be corrupted with partial frames. Without Pravega, writes to SMB and NFS are generally not atomic.

3. Pravega does not make a video frame available to readers until the entire video frame has been durably persisted.

4. Pravega provides low end-to-end latency (15-20 milliseconds) between a camera writing a frame and a video player displaying it. This low latency is impossible to achieve with file-only workflows due to limitations of locking, polling, NFS, and SMB.

5. Older frames of video streams can be automatically truncated based on time or size limits.

6. The same API can be used to read live video or historical data from years ago, regardless of Long Term Storage technology. Pravega will serve data cached in memory if available or it will read from Long Term Storage.

### Installation

Run the following command to start.

```
$ yarn

# Build frontend for production
$ yarn build

# Build for production and view the bundle analyzer report.
$ yarn build --report

# Build backend for production
$ cd src-backend && cargo build --release
```

### Development

-   Install dev tools

    ```
    cargo install cargo-watch
    rustup component add clippy
    rustup component add rustfmt
    apt-get install libsqlite3-dev
    cargo install diesel_cli --no-default-features --features sqlite
    ```

-   Generate dev db

    ```
    cd src-backend
    diesel setup --database-url dev.db
    diesel migration run --database-url dev.db
    ```

-   Serve frontend with hot reload at 0.0.0.0:5173 by default.

    ```
    yarn dev --host
    ```

-   Serve backend with hot reload at 0.0.0.0:8080

    ```
    yarn backend
    ```

-   Linting
    ```
    yarn lint
    yarn clippy
    ```

### License

[MIT](https://github.com/epicmaxco/vuestic-admin/blob/master/LICENSE) license.

### Credits

1. Free and Beautiful [vuestic-admin](https://github.com/epicmaxco/vuestic-admin) template
2. [GStreamer](https://gitlab.freedesktop.org/gstreamer/gstreamer) framework for streaming media
