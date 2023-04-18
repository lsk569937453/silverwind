# Silverwind-下一代高性能云原生反向代理/负载均衡调度器
Silverwind 是一个高性能的反向代理/负载均衡调度器。它同时也可以在K8S集群中用作入口控制器。
## Sivlverwind-Dashboard
### docker-compose 启动
通过如下命令来启动Sivlverwind的Dashboard  
docker-compose.yaml的文件如下所示
```
version: "3.9"
services:
  silverwind-dashboard:
    image: lsk569937453/silverwind-dashboard:0.0.7
    container_name: silverwind-dashboard
    ports:
      - "4486:4486"

  silverwind:
      image: lsk569937453/silverwind:0.0.7
      container_name: silverwind
      ports:
        - "6980:6980"
      environment:
        ADMIN_PORT: 6980
```
您执行**docker-compose up** 命令后，可以在浏览器中打开[主页](http://localhost:4486/index.html)来查看Silverwind的仪表盘。
## 为什么我们选择 Silverwind&&Silverwind的优点
### 基准测试
我们针对当前主流的代理/负载均衡器(NGINX, Envoy, and Caddy)做了性能测试。测试结果在[这里](https://github.com/lsk569937453/silverwind/blob/main/benchmarks-zh_CN.md)。
测试结果表明在相同的机器配置下(4核8G),在某些指标上(每秒请求数,平均响应时间),Silverwind的数据与NGINX, Envoy水平接近。
在请求延迟上，Silverwind的数据要明显好于NGINX和Envoy。

### 所有的基础功能全都是原生语言开发-速度快
Silverwind不止是一个反向代理/负载均衡器，而且是一个API网关。作为一个API网关，Silverwind将会涵盖所有的基础功能(黑白名单/授权/熔断限流/灰度发布
,蓝绿发布/监控/缓存/协议转换)。

与其他的网关相比，Silverwind的优点是涵盖API网关所有的基础服务，并且性能高。其次，Silverwind的动态配置接近实时。每次修改完配置，会在5秒内生效(接近实时)。

### Kong
Kong的免费限流插件[不准确]( https://github.com/Kong/kong/issues/5311)。如果想要实现更准确的限流，我们不得不买企业版的Kong。


### Envoy
Envoy没有内嵌限流功能。Envoy提供了限流接口让用户自己实现。目前github上使用最多的是这个[项目](https://github.com/envoyproxy/ratelimit)。
第一个缺点是该项目只支持固定窗口限流。固定窗口限流算法的坏处是不支持突发流量。  
第二个缺点是每次请求Envoy都会通过使用grpc去请求限流集群。相比内嵌的限流算法，这其实额外的增加了一次网络跃点。 

## 动态配置
您可以通过Rest API更改配置。并且新配置将在**5 秒内**生效。
## 编译或者下载发行版
### 编译
请先安装rust，然后执行下面的命令。
```
cd rust-proxy
cargo build --release
```
你可以在target/release目录下找到Silverwind.
### 下载发行版
从[这里](https://github.com/lsk569937453/silverwind/releases)下载发行版.
## 配置文件介绍
### 配置Silverwind作为http代理
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
Silverwind将会监听9969端口然后转发流量到 http://localhost:8888/,http://localhost:9999/.http://localhost:7777/ 。
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
Silverwind将会监听4486端口然后转发流量到 httpbin.org:443。
### 启动:
#### Windows下启动
```
$env:CONFIG_FILE_PATH='D:\code\app_config.yaml'; .\rust-proxy.exe
```
或者也可以无配置文件启动。
```
.\rust-proxy.exe
```
## Rest Api
### 修改配置
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
### 获取配置
```
GET /appConfig HTTP/1.1
Host: 127.0.0.1:8870
```
### 更新路由
```
PUT /route HTTP/1.1
Host: 127.0.0.1:8870
Content-Type: application/json
Content-Length: 629

{
    "route_id": "90c66439-5c87-4902-aebb-1c2c9443c154",
    "host_name": null,
    "matcher": {
        "prefix": "/",
        "prefix_rewrite": "ssss"
    },
    "allow_deny_list": null,
    "authentication": null,
    "anomaly_detection": null,
    "liveness_config": null,
    "health_check": null,
    "ratelimit": null,
    "route_cluster": {
        "type": "RandomRoute",
        "routes": [
            {
                "base_route": {
                    "endpoint": "http://127.0.0.1:10000",
                    "try_file": null,
                    "is_alive": null
                }
            }
        ]
    }
}
```
### 删除路由
```
DELETE /route/90c66439-5c87-4902-aebb-1c2c9443c154 HTTP/1.1
Host: 127.0.0.1:8870
```
## <span id="api-gateway">API网关中的基础功能</span>
![alt tag](https://raw.githubusercontent.com/lsk569937453/image_repo/main/api-gateway.png)
## Silverwind已经实现了如下功能
* IP 黑白名单
* 授权(Basic Auth,ApiKey Auth)
* 限流(Token Bucket,Fixed Window)
* 路由
* 负载均衡(论询，随机，基于权重,基于Header)
* 动态配置(Rest Api)
* 健康检查&异常检测
* 免费Https证书
* 控制面板
* 监控(Prometheus)
## 将来会实现的功能
- [ ] 协议转换
- [ ] 缓存
