- Adjust the following param according to the total memory
```
-Dpravegaservice.cache.size.max=3221225472
-Xmx4g
-XX:MaxDirectMemorySize=4G
```

- Build devkit

```
docker build -t finalnvr/devkit --build-arg HTTP_PROXY="http://172.17.0.1:19000" --build-arg HTTPS_PROXY="http://172.17.0.1:19000"  -f devkit.Dockerfile .
```

- Develop FinalNVR in devkit container

```
xhost +
docker run -it -v /mnt/data/projects/finalnvr/:/finalnvr --net=host --env="DISPLAY" --volume="$HOME/.Xauthority:/root/.Xauthority:rw" finalnvr/devkit bash
```
