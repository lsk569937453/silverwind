

# Benchmarks
Tests performance of various proxies/load balancers. Based on the [Proxy-Benchmarks](https://github.com/NickMRamirez/Proxy-Benchmarks).

We test the following proxies:

* Caddy
* Envoy
* NGINX
* Silverwind

## Setup
We use the **docker-compose** to do the performance test.Install the docker on your computer and confirm that the your computer have enough cpu and memory.There are three services in the docker-compose including the hey(Testing Tool),proxy and the backend.We limit **the cpu cores(4 core) and memory(8GB)** for the service. 

Our testing environment is based on the PC.And the cpu of the PC is i5 13600,the memory of the PC is 32GB.
## Results using Hey
![alt tag](https://raw.githubusercontent.com/lsk569937453/image_repo/main/requestForSecond.png)
![alt tag](https://raw.githubusercontent.com/lsk569937453/image_repo/main/average-response-time.png)

![alt tag](https://raw.githubusercontent.com/lsk569937453/image_repo/main/Latency%20distribution.png)

Graphs created using [https://www.rapidtables.com/tools/bar-graph.html](https://www.rapidtables.com/tools/bar-graph.html)
## Nginx(1.23.3)
```
 hey -n 100000 -c 250 -m GET http://nginx:80/

Summary:
  Total:        2.3592 secs
  Slowest:      0.1100 secs
  Fastest:      0.0002 secs
  Average:      0.0058 secs
  Requests/sec: 42387.9092

  Total data:   14400000 bytes
  Size/request: 144 bytes

Response time histogram:
  0.000 [1]     |
  0.011 [93403] |■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■
  0.022 [1568]  |■
  0.033 [3424]  |■
  0.044 [1354]  |■
  0.055 [0]     |
  0.066 [0]     |
  0.077 [217]   |
  0.088 [22]    |
  0.099 [2]     |
  0.110 [9]     |


Latency distribution:
  10% in 0.0025 secs
  25% in 0.0032 secs
  50% in 0.0042 secs
  75% in 0.0053 secs
  90% in 0.0074 secs
  95% in 0.0222 secs
  99% in 0.0350 secs

Details (average, fastest, slowest):
  DNS+dialup:   0.0000 secs, 0.0002 secs, 0.1100 secs
  DNS-lookup:   0.0001 secs, 0.0000 secs, 0.0839 secs
  req write:    0.0000 secs, 0.0000 secs, 0.0825 secs
  resp wait:    0.0056 secs, 0.0001 secs, 0.0796 secs
  resp read:    0.0001 secs, 0.0000 secs, 0.0795 secs

Status code distribution:
  [200] 100000 responses
```
## SilverWind
```
hey -n 100000 -c 250 -m GET http://silverwind:6667

Summary:
  Total:        2.1140 secs
  Slowest:      0.0540 secs
  Fastest:      0.0002 secs
  Average:      0.0052 secs
  Requests/sec: 47303.5740

  Total data:   13800000 bytes
  Size/request: 138 bytes

Response time histogram:
  0.000 [1]     |
  0.006 [62378] |■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■
  0.011 [37035] |■■■■■■■■■■■■■■■■■■■■■■■■
  0.016 [336]   |
  0.022 [0]     |
  0.027 [0]     |
  0.032 [0]     |
  0.038 [6]     |
  0.043 [110]   |
  0.049 [1]     |
  0.054 [133]   |


Latency distribution:
  10% in 0.0030 secs
  25% in 0.0039 secs
  50% in 0.0050 secs
  75% in 0.0062 secs
  90% in 0.0074 secs
  95% in 0.0082 secs
  99% in 0.0100 secs

Details (average, fastest, slowest):
  DNS+dialup:   0.0000 secs, 0.0002 secs, 0.0540 secs
  DNS-lookup:   0.0000 secs, 0.0000 secs, 0.0401 secs
  req write:    0.0000 secs, 0.0000 secs, 0.0032 secs
  resp wait:    0.0051 secs, 0.0002 secs, 0.0414 secs
  resp read:    0.0000 secs, 0.0000 secs, 0.0041 secs

Status code distribution:
  [200] 100000 responses
```
## Envoy(1.22.8)
```
 hey -n 100000 -c 250 -m GET http://envoy:8050

Summary:
  Total:        2.3919 secs
  Slowest:      0.0882 secs
  Fastest:      0.0001 secs
  Average:      0.0057 secs
  Requests/sec: 41808.3304

  Total data:   24600000 bytes
  Size/request: 246 bytes

Response time histogram:
  0.000 [1]     |
  0.009 [92768] |■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■
  0.018 [1472]  |■
  0.027 [45]    |
  0.035 [0]     |
  0.044 [350]   |
  0.053 [3464]  |■
  0.062 [1655]  |■
  0.071 [196]   |
  0.079 [45]    |
  0.088 [4]     |


Latency distribution:
  10% in 0.0009 secs
  25% in 0.0015 secs
  50% in 0.0026 secs
  75% in 0.0043 secs
  90% in 0.0071 secs
  95% in 0.0474 secs
  99% in 0.0550 secs

Details (average, fastest, slowest):
  DNS+dialup:   0.0000 secs, 0.0001 secs, 0.0882 secs
  DNS-lookup:   0.0001 secs, 0.0000 secs, 0.0696 secs
  req write:    0.0000 secs, 0.0000 secs, 0.0661 secs
  resp wait:    0.0054 secs, 0.0001 secs, 0.0637 secs
  resp read:    0.0002 secs, 0.0000 secs, 0.0617 secs

Status code distribution:
  [200] 100000 responses
```
## Caddy(2.6.4)
```
hey -n 100000 -c 250 -m GET http://caddy:80/

Summary:
  Total:        19.3878 secs
  Slowest:      1.2016 secs
  Fastest:      0.0001 secs
  Average:      0.0444 secs
  Requests/sec: 5157.8768

  Total data:   20866020 bytes
  Size/request: 208 bytes

Response time histogram:
  0.000 [1]     |
  0.120 [93685] |■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■
  0.240 [4394]  |■■
  0.361 [1205]  |■
  0.481 [375]   |
  0.601 [154]   |
  0.721 [84]    |
  0.841 [33]    |
  0.961 [12]    |
  1.081 [7]     |
  1.202 [50]    |


Latency distribution:
  10% in 0.0021 secs
  25% in 0.0046 secs
  50% in 0.0115 secs
  75% in 0.0778 secs
  90% in 0.1022 secs
  95% in 0.1773 secs
  99% in 0.3031 secs

Details (average, fastest, slowest):
  DNS+dialup:   0.0000 secs, 0.0001 secs, 1.2016 secs
  DNS-lookup:   0.0000 secs, 0.0000 secs, 0.0520 secs
  req write:    0.0000 secs, 0.0000 secs, 0.0456 secs
  resp wait:    0.0441 secs, 0.0001 secs, 1.2015 secs
  resp read:    0.0002 secs, 0.0000 secs, 0.0460 secs

Status code distribution:
  [200] 99362 responses
  [502] 638 responses

```