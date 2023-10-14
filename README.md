## FinalNVR

FinalNVR is a ready-to-use NVR system that allows user to view, store and analyze video streams from IP cameras in real time.

FinalNVR's backend storage is [pravega](https://github.com/pravega/pravega) which is a decent streaming storage system for video streams.

### Installation

Run the following command to start.

```
$ yarn

# serve with hot reload at localhost:8080 by default.
$ yarn dev --host

# build for production
$ yarn build

# build for production and view the bundle analyzer report.
$ yarn build --report
```

export HTTP_PROXY=http://172.17.0.1:19000
export HTTPS_PROXY=http://172.17.0.1:19000
cargo install cargo-watch

### License

[MIT](https://github.com/epicmaxco/vuestic-admin/blob/master/LICENSE) license.
