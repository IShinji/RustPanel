# Milestone 0:Alpine + musl 在 128MB 内存/128MB 磁盘下的可行性实测

本记录对应 `docs/planning` 之外的一次性可行性验证,不是长期维护的指南,结论用于
决定是否继续投入 ServiceManager/PackageManager/activate 监督者这一整套改造。

## 范围说明

128MB 磁盘目标**只在 LXC/OpenVZ 这类共享宿主机内核、不在容器内落盘内核的 NAT 小鸡
上成立**——KVM 虚拟机跑 Alpine 自己还要装内核 + initramfs,落盘体积通常 60-90MB,
跟 128MB 预算冲突。本次实测用 Docker 近似 LXC 的"共享内核、不含内核镜像"特性来测
磁盘占用和内存行为;真正的 OpenRC-as-PID1、开机自启等行为需要在后续
OpenRC 实现里用真实 LXC 容器验证,不在本次范围。

## 实测环境

- 宿主机:macOS,Apple Silicon(aarch64)
- 工具:Docker Desktop(`rust:alpine` 构建镜像、`alpine:3.24` 运行时镜像)
- 二进制:`cargo build --release --no-default-features`(micro 档,等同生产用的
  "micro" 构建)

## 1. musl 交叉编译

`rust:alpine` 镜像(musl-native 工具链)内直接 `apk add musl-dev protobuf-dev` 后
跑 `cargo build --release --no-default-features`:

- **一次性构建成功,零错误**。没有遇到预期中 `protoc-bin-vendored` 的 musl 兼容性
  问题(`protobuf-dev` 提供的系统 protoc 直接可用)。
- 产出二进制:**13.0MB**,完全静态链接(`ldd` 报 "Not a valid dynamic program",
  确认没有任何动态依赖,包括 musl 自己的动态加载器)。
- 对比同配置 glibc 构建(此前在 Debian 环境测得 13.4MB):**musl 版本还略小**,
  没有体积代价。

结论:**musl 交叉编译没有遇到任何阻塞**,项目一直坚持 rustls/ring、不碰 openssl
native-tls 的路线在这里直接受益。

## 2. 磁盘占用

逐步测量(`du -sx /`,单位 KB):

| 阶段 | 占用 | 增量 |
|------|------|------|
| Alpine 3.24 minirootfs | 9,820 KB (~9.6MB) | — |
| + openrc + openssh + chrony | 18,496 KB (~18.1MB) | +8.5MB |
| + sqlite(CLI 工具,代表数据库运行时依赖) | 24,848 KB (~24.3MB) | +6.4MB |
| + RustPanel 二进制(13.0MB)+ 最小运行态(空 sqlite 库 + 证书目录) | **38,208 KB (~37.3MB)** | +13.0MB |

**总计约 37-38MB**,距离 Milestone 0 定的止损线(>70MB 回去重新评估)还有将近
一倍的余量,128MB 预算里装完 OS+面板后还剩 **~90MB** 给站点数据、日志、备份、
证书和 swap。

## 3. 内存占用

二进制在 `docker run -m 128m --memory-swap 128m`(128MB 内存上限,不给 swap)下
直接起:

| 阶段 | RSS |
|------|-----|
| 刚启动,空闲 | 4.9MB |
| 连续 50 次 HTTP 请求(含前端静态资源)后 | 9.1MB |
| 再空闲 15 秒 | 9.0MB(稳定,无持续增长) |

前端(`rust-embed` 内嵌的 dist,含中文 `lang="zh-CN"` 页面)在容器里正常经 HTTP 返回,
确认嵌入资源在 musl 构建下没有问题。

**远低于止损线(>90MB 回去重新评估)**——空载和轻负载下只用了 128MB 预算的 4-7%。
本次没有覆盖 ACME 签发、WebSocket 终端会话、zip 打包这几个本该更吃内存的场景
(受限于本次只做最小可行性验证,没有搭建完整站点),后续 OpenRC 里程碑里的端到端
测试需要补上这几项的实测数据,但基于目前的巨大余量,没有看到会落到止损线附近的
迹象。

## 结论

**128MB 内存 + 128MB 磁盘(LXC/OpenVZ + Alpine)这个目标现实可行,且余量相当充足**
(磁盘用了约 30%,内存空闲/轻载只用了 4-7%)。继续按计划推进 ServiceManager /
activate 监督者 / OpenRC 支持这几个里程碑,不需要回头重新评估目标范围。

## 已知局限(本次未覆盖,留给后续里程碑)

- 没有用真实 LXC 容器验证 OpenRC 作为 PID 1 的行为(开机自启、`rc-service`/
  `supervise-daemon` 的实际重启/崩溃恢复表现)。
- 没有测 ACME 签发、WebSocket 终端、zip 打包这几个相对重的场景下的 RSS 峰值。
- 本次构建产出的是 aarch64 二进制(受限于实测用的 Apple Silicon 宿主机);NAT VPS
  市场主要是 x86_64,后续 CI 里的 musl 构建需要用 `x86_64-unknown-linux-musl`
  目标验证一遍,预期结果应该一致(静态链接、无 glibc 依赖,架构差异不影响这里的
  体积/内存结论)。
