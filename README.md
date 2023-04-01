# Silverwind-The Next Generation High-Performance Proxy
The Silverwind is a high-performance reverse proxy/load balancer. And it could be also used as the ingress
controller in the k8s.
## Why we chose Sivlverwind
### Benchmarks
We do the performance testing between several popular proxies including NGINX, Envoy, and Caddy. The benchmarks show [here](https://github.com/lsk569937453/silverwind/blob/main/benchmarks.md).
The testing result shows that our proxy has better performance.

### Rate Limit is not accurate or too slow
We think the API gateway should contain several functions like the rate limit, etc. The rate limit plugin
on open source is not accurate. We have to buy the Kong Enterprise https://github.com/Kong/kong/issues/5311 if
we want to do the accurate rate limiting.

The rate limit in the envoy is not built-in and the envoy will hit the rate limit over the network. So it will
increase the network hops.

The Sivlverwind has a built-in rate limiting.And we will maintain it free. If you have some advice, you could  post it 
in GitHub issues.
## Dynamic Configuration
### Single Silver-wind
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
## Dashboard For Silverwind
## Future
- [ ] Protocol Translation
- [ ] Caching
- [ ] Monitoring