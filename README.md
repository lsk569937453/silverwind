# Silverwind-The Next Generation High-Performance Proxy
English  [简体中文](./README-zh_CN.md) 

The Silverwind is a high-performance reverse proxy/load balancer. And it could be also used as the ingress
controller in the k8s.
## Sivlverwind-Dashboard
###
Start the Sivlverwind-Dashboard over docker-compose.  
The docker-compose.yaml is like following:
```
version: "3.9"
services:
  silverwind-dashboard:
    image: lsk569937453/silverwind-dashboard:0.0.4
    container_name: silverwind-dashboard
    ports:
      - "4486:4486"

  silverwind:
      image: lsk569937453/silverwind:0.0.4
      container_name: silverwind
      ports:
        - "6980:6980"
      environment:
        ADMIN_PORT: 6980
```
You could check the [main page](http://localhost:4486/index.html) for Silverwind -Dashboard after you execute the **docker-compose up** command.
## Why we chose Sivlverwind
### Benchmarks
We do the performance testing between several popular proxies including NGINX, Envoy, and Caddy. The benchmarks show [here](https://github.com/lsk569937453/silverwind/blob/main/benchmarks.md).

The test results show that under the same machine configuration (4 cores 8G), in some indicators (requests per second, average response time), the data of Silverwind is almost the same as the  NGINX and Envoy.
In terms of request latency, Silverwind is better than  NGINX and Envoy.

### All basic functions are developed in native language - fast
Silverwind is not only a reverse proxy/load balancer, but also an API gateway. As an API gateway, Silverwind will cover all basic functions (black and white list/authorization/fuse limit/gray release
, blue-green publishing/monitoring/caching/protocol conversion).

Compared with other gateways, Silverwind has the advantage of covering all the basic services of the API gateway, and has high performance. Second, Silverwind's dynamic configuration is close to real-time. Every time the configuration is modified, it will take effect within 5 seconds (close to real-time).

### Kong
The free Ratelimiting plugin for Kong is [inaccurate](https://github.com/Kong/kong/issues/5311). If we want to achieve more accurate Ratelimiting, we have to buy the enterprise version of Kong.

### Envoy
Envoy does not have built-in ratelimiting. Envoy provides a ratelimiting interface for users to implement by themselves. Currently the most used is this [project](https://github.com/envoyproxy/ratelimit).
The first disadvantage is that the project only supports fixed-window ratelimiting. The disadvantage of the fixed window ratelimiting is that it does not support burst traffic.
The second disadvantage is that every time Envoy is requested, it will use grpc to request the ratelimiting cluster. Compared with the built-in current limiting algorithm, this actually adds an additional network hop.

## Dynamic Configuration
You could change the configuration over the rest API. And the new configuration will have an effect **in 5 seconds**.

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
        - base_route:
            endpoint: http://localhost:8888/
            try_file: null
        - base_route:
            endpoint: http://localhost:9999/
            try_file: null
        - base_route:
            endpoint: http://localhost:7777/
            try_file: null
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
        - base_route:
            endpoint: httpbin.org:443
            try_file: null
```
### Setup:
#### Windows Startup
```
$env:CONFIG_FILE_PATH='D:\code\app_config.yaml'; .\rust-proxy.exe
```
Or you could start without the config file like the following:
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
            "cert_str": null,
            "key_str": null,
            "routes": [
                {
                    "matcher": {
                        "prefix": "ss",
                        "prefix_rewrite": "ssss"
                    },
                    "allow_deny_list": null,
                    "route_cluster": {
                        "type": "WeightBasedRoute",
                        "routes": [
                            {
                                "base_route": {
                                    "endpoint": "http://localhost:10000",
                                    "try_file": null
                                }
                            }
                        ]
                    }
                },
                {
                    "matcher": {
                        "prefix": "sst",
                        "prefix_rewrite": "ssss"
                    },
                    "allow_deny_list": null,
                    "route_cluster": {
                        "type": "WeightBasedRoute",
                        "routes": [
                            {
                                "base_route": {
                                    "endpoint": "http://localhost:9898",
                                    "try_file": null
                                }
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
## <span id="api-gateway">The Base Function in Api Gateway</span>
![alt tag](https://raw.githubusercontent.com/lsk569937453/image_repo/main/api-gateway.png)
## Silverwind has implemented the following functions:
* IP Allow-and-Deny list
* Authentication(Basic Auth,ApiKey Auth)
* Rate limiting(Token Bucket,Fixed Window)
* Routing
* Load Balancing(Poll,Random,Weight,Header Based)
* Dynamic Configuration(Rest Api)
* Dashboard For Silverwind
* Monitoring(Prometheus)
## Future
- [ ] Protocol Translation
- [ ] Caching
