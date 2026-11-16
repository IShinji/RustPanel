# Runner 出网排查与修复(self-hosted / OrbStack)

Release CI 跑在 self-hosted ARM64 runner 上(OrbStack 里的 Ubuntu,kernel 里带 `orbstack` 标记)。
历史上多次出现「构建本身没问题、但 job 卡在下载 action 或推送镜像」的失败,典型报错:

```
Failed to download action 'https://codeload.github.com/actions/checkout/...'
  Error: The SSL connection could not be established
Error response from daemon: Get "https://ghcr.io/v2/": EOF
##[error]Failed to CreateArtifact: Unable to make request: ECONNRESET
```

## 先诊断

```bash
gh workflow run runner-network-doctor.yml --ref main
# 若已知代理地址,可以顺带验证代理是否真的通:
gh workflow run runner-network-doctor.yml --ref main -f proxy=http://192.168.8.179:7890
gh run watch "$(gh run list --workflow runner-network-doctor.yml --limit 1 --json databaseId --jq '.[0].databaseId')"
```

这个 workflow 不 checkout、不下载任何 action,所以 runner 出网再差也能跑。报告里看四段:

1. `--- proxy env ---`:空说明 **job 里根本没有代理环境变量**(runner 服务没继承 Mac 上的代理)
2. `#### api.github.com` 等:哪一跳 TLS 失败(`SSL_ERROR_SYSCALL` = 握手被重置)
3. `--- mtu probe (DF) ---`:MTU 偏小也会表现为 TLS 握手随机失败
4. `#### 宿主侧常见代理端口`:空说明宿主代理没暴露到 runner(没开「允许局域网连接」或端口不对)

## 修复:把 runner 和 Docker 都指到代理

Mac 侧:`ClashX / Surge / Mihomo` 打开「允许局域网连接」,记下 HTTP 端口(常见 7890)。
OrbStack 里访问宿主用 `host.internal`,也可以用 Mac 的局域网 IP(如 `192.168.8.179`)。

runner 服务(ACTIONS 里跑的 node/git/curl 都吃这套环境变量):

```bash
sudo systemctl edit "actions.runner.<owner>-<repo>.<runner-name>.service"
# 写入:
# [Service]
# Environment=HTTP_PROXY=http://192.168.8.179:7890
# Environment=HTTPS_PROXY=http://192.168.8.179:7890
# Environment=NO_PROXY=localhost,127.0.0.1,::1,host.internal,.internal
sudo systemctl daemon-reload
sudo systemctl restart actions.runner.<owner>-<repo>.<runner-name>.service
```

Docker daemon(推/拉 ghcr.io 走 daemon,不吃 runner 的环境变量):

```bash
sudo mkdir -p /etc/systemd/system/docker.service.d
sudo tee /etc/systemd/system/docker.service.d/proxy.conf >/dev/null <<'EOF'
[Service]
Environment="HTTP_PROXY=http://192.168.8.179:7890"
Environment="HTTPS_PROXY=http://192.168.8.179:7890"
Environment="NO_PROXY=localhost,127.0.0.1,::1"
EOF
sudo systemctl daemon-reload
sudo systemctl restart docker
```

## 验证

```bash
gh workflow run runner-network-doctor.yml --ref main -f proxy=http://192.168.8.179:7890
```

报告里 `[proxy]` 开头的段落全部 `http=200/301` 就说明代理通了;然后重跑 Release CI 的失败 job 即可。

## 仓库侧已经做的减依赖(减少对这套网络的依赖)

- amd64 产物改成原生 arm64 交叉编译,不再靠 QEMU 模拟
- 发布不再 `docker pull` 自己刚推的镜像(改成 `load: true` + 本地 `docker cp`)
- 后端产物改成 runner 本地目录交接(`RELEASE_ARTIFACT_ROOT`),不走 artifact 上传/下载
- 发布 job 去掉 `actions/setup-node`,改用 runner 自带 `bun` 跑 CI 脚本

剩下的必需网络依赖只有:下载 actions(`codeload.github.com`)、推送镜像(`ghcr.io`)、
发布 Release(`api.github.com`)。这三个都得靠上面的代理修复。
