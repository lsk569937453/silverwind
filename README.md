# Silverwind-The Next Generation High Performance Proxy
The silverwind is developed by the rust.It could be used as the reverse proxy or load banlancer.
## Benchmarks
The benchmarks shows [here](https://github.com/lsk569937453/silverwind/blob/main/benchmarks.md).
## Dynamic Configuration 
### Single Silverwind
You could change the configuration over the rest api.And the new configuration will have effect **in 5 seconds**.
### Silverwind Cluster(future)
There are two plans here.
* The Silverwind will poll the new config from the the other interface over the grpc/rest api.The user could implement their own api.
* The Silverwind will poll the new config from the database(mysql/postgresql).And we will offer the web-ui for the user to change the configuration.

## Compile or Download the release
### Compile 
You have to install the rust first.
```
cd rust-proxy
cargo build --release
```
You could get the release in the target/release.
### Download the release
Download the release from the [website](https://github.com/lsk569937453/silverwind/releases).
## Config Introduction
### Silverwind as the http proxy
```
- listen_port: 9969
  service_config:
    server_type: HTTP
    routes:
    - matcher:
        prefix: /
        prefix_rewrite: ssss
      route_cluster:
        type: RandomRoute
        routes:
        - endpoint: http://localhost:8888/
          weight: 100
        - endpoint: http://localhost:9999/
          weight: 100
        - endpoint: http://localhost:7777/
          weight: 100
```
The proxy will listen the 9969 port and forward the traffic to the http://localhost:8888/,http://localhost:9999/.http://localhost:7777/.
### Silverwind as the tcp proxy
```
- listen_port: 4486
  service_config:
    server_type: TCP
    routes:
    - matcher:
        prefix: "/"
        prefix_rewrite: ssss
      route_cluster:
        type: RandomRoute
        routes:
        - endpoint: httpbin.org:443
          weight: 100
```
### Setup:
#### Windows Startup
```
$env:CONFIG_FILE_PATH='D:\code\app_config.yaml'; .\rust-proxy.exe
```
Or you could start without the config file like following:
```
.\rust-proxy.exe
```
## Rest Api
### Change the routes
```
POST /appConfig HTTP/1.1
Host: 127.0.0.1:8870
Content-Type: application/json
Content-Length: 1752

[
    {
        "listen_port": 4486,
        "service_config": {
            "server_type": "HTTP",
            "routes": [
                {
                    "matcher": {
                        "prefix": "/",
                        "prefix_rewrite": "ssss"
                    },
                    "route_cluster": {
                        "type": "RandomRoute",
                        "routes": [
                            {
                                "endpoint": "/dist",
                                "weight": 100
                            }
                        ]
                    }
                }
            ]
        }
    },
    {
        "listen_port": 9969,
        "service_config": {
            "server_type": "HTTP",
            "routes": [
                {
                    "matcher": {
                        "prefix": "/",
                        "prefix_rewrite": "ssss"
                    },
                    "route_cluster": {
                        "type": "WeightRoute",
                        "routes": [
                            {
                                "endpoint": "http://localhost:7777/",
                                "weight": 100
                            },
                            {
                                "endpoint": "http://localhost:8888/",
                                "weight": 100
                            },
                            {
                                "endpoint": "http://localhost:9999/",
                                "weight": 100
                            }
                        ]
                    }
                }
            ]
        }
    }
]
```
### Get the routes
```
GET /appConfig HTTP/1.1
Host: 127.0.0.1:8870
```
## Silverwind has implemented the following functions:
* Routing
* Load Balancing(Poll,Random,Weight)
* Dynamic Configuration(Rest Api)
## Future
- [&check;] **Allow-and-Deny list**
- [&check;] **Grayscale publishing & A/B Testing**
- [&check;] **Authentication**
- [&check;] **Rate limiting**
- [ ] Protocol Translation
- [ ] Caching
- [ ] Monitoring