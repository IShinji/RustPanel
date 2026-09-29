import type { NetworkMessages } from "../zh-CN/network";

export const network: NetworkMessages = {
  subtitle: "Manage the NAT VPS's 20-port public port budget and the public IPv6 address pool",
  refresh: "Refresh",
  portRangeError: "Port must be between 1 and 65535",
  ownerRequired: "Enter who this port is reserved for",
  portReserved: "Port {port} reserved",
  portReleased: "Port {port} released",

  natBudgetTitle: "NAT Port Budget",
  natBudgetDesc:
    "Record who is using each port to avoid collisions when installing new software. Register the panel, SSH, and any live services.",
  port: "Port",
  owner: "Reserved for",
  description: "Description",
  protocol: "Protocol",
  ownerPlaceholder: "e.g. panel / site:my-blog",
  optional: "Optional",
  reserve: "Reserve",
  noPortsRegistered: "No ports registered yet",
  registeredAt: "Registered",
  actions: "Actions",
  release: "Release",

  ipv6PoolTitle: "Public IPv6 Address Pool",
  ipv6PoolDesc:
    "On a NAT VPS, IPv6 is the key to bypassing the 20-port limit — bind each site directly to a v6 address without using a NAT port.",
  detectedPrefixes: "Detected public prefixes",
  noIpv6Detected: "No public IPv6 address detected (IPv6 may be disabled, or only link-local)",
  address: "Address",
  prefix: "Prefix",
  interface: "Interface",
  type: "Type",
  global: "Global",
  local: "Local",

  capabilityTitle: "Host Capability Detection",
  capabilityDesc: "Detected once at boot, refreshed every hour",
  capOpenvz: "OpenVZ container",
  capContainer: "Inside Docker / LXC",
  capDockerDaemon: "Docker daemon",
  capDockerAvailable: "Docker available",
  capOverlay2: "overlay2 filesystem",
  capFuse: "FUSE",
  capIptables: "iptables binary",
  capNfNat: "nf_nat module",
  capSwap: "Swap",
  capBbr: "BBR congestion control",
  capCgroupsV2: "cgroups v2",
  capUserNamespaces: "user namespaces",
  kernelPrefix: "Kernel: "
};
