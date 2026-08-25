import { Input } from "../components/form-controls";
import { Badge } from "../components/ui/badge";
import { Button as UIButton } from "../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../components/ui/card";
import { Label as UILabel } from "../components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../components/ui/select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow as UITableRow } from "../components/ui/table";
import { DnsProvider, type DnsRecord } from "../gen/rustpanel/v1/dns_pb";
import { User, UserRole } from "../gen/rustpanel/v1/user_pb";
import { formatBytes, safeError } from "../lib/format";
import { type Clients } from "../lib/rpc";
import { BarChart3, Plus, RefreshCw, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

type DnsFormState = {
  id: string;
  type: string;
  name: string;
  content: string;
  ttl: string;
  proxied: boolean;
};

const EMPTY_DNS_FORM: DnsFormState = {
  id: "",
  type: "A",
  name: "",
  content: "",
  ttl: "1",
  proxied: false
};

export function DnsPage({ clients }: { clients: Clients }) {
  const [zoneId, setZoneId] = useState("");
  const [apiToken, setApiToken] = useState("");
  const [configured, setConfigured] = useState(false);
  const [records, setRecords] = useState<DnsRecord[]>([]);
  const [form, setForm] = useState<DnsFormState>(EMPTY_DNS_FORM);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");

  const loadConfig = useCallback(async () => {
    try {
      const response = await clients.dns.getDnsConfig({});
      setZoneId(response.zoneId);
      setConfigured(response.configured);
      setError("");
      return response.configured;
    } catch (err) {
      setError(safeError(err));
      return false;
    }
  }, [clients]);

  const loadRecords = useCallback(async () => {
    try {
      const response = await clients.dns.listDnsRecords({});
      setRecords(response.records);
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  }, [clients]);

  useEffect(() => {
    void loadConfig().then((ok) => {
      if (ok) void loadRecords();
    });
  }, [loadConfig, loadRecords]);

  const saveConfig = async () => {
    try {
      await clients.dns.setDnsConfig({
        provider: DnsProvider.CLOUDFLARE,
        zoneId: zoneId.trim(),
        apiToken: apiToken.trim()
      });
      setApiToken("");
      setMessage("DNS 配置已保存");
      setError("");
      const ok = await loadConfig();
      if (ok) void loadRecords();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const saveRecord = async () => {
    try {
      await clients.dns.upsertDnsRecord({
        record: {
          id: form.id.trim(),
          type: form.type.trim(),
          name: form.name.trim(),
          content: form.content.trim(),
          ttl: Number(form.ttl) || 1,
          proxied: form.proxied
        }
      });
      setForm(EMPTY_DNS_FORM);
      setMessage("解析记录已保存");
      setError("");
      void loadRecords();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const deleteRecord = async (id: string) => {
    if (!window.confirm("确认删除该解析记录?")) return;
    try {
      await clients.dns.deleteDnsRecord({ id });
      setMessage("解析记录已删除");
      setError("");
      void loadRecords();
    } catch (err) {
      setError(safeError(err));
    }
  };

  return (
    <section className="flex flex-col gap-5">
      <header className="flex flex-col gap-1">
        <h1 className="text-2xl font-semibold tracking-tight m-0">DNS 解析托管</h1>
        <p className="text-sm text-muted-foreground m-0">
          对接 Cloudflare API 管理解析记录(亦可服务 ACME DNS-01 自动加 TXT)。API token 仅存于后端,0600 落盘。
        </p>
      </header>

      {error && (
        <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive whitespace-pre-line">
          {error}
        </div>
      )}
      {message && !error && (
        <div className="rounded-md border border-success/40 bg-success/10 px-3 py-2 text-sm text-success">
          {message}
        </div>
      )}

      <Card>
        <CardHeader>
          <CardTitle>服务商配置</CardTitle>
          <CardDescription>
            Cloudflare · {configured ? "已配置(token 留空表示保留原值)" : "未配置"}
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid gap-3 sm:grid-cols-2">
            <Input label="Zone ID" value={zoneId} onChange={setZoneId} />
            <Input
              label="API Token"
              type="password"
              value={apiToken}
              onChange={setApiToken}
            />
          </div>
          <div className="mt-3">
            <UIButton size="sm" onClick={() => void saveConfig()}>
              保存配置
            </UIButton>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{form.id ? "编辑解析记录" : "新增解析记录"}</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
            <Input label="类型 (A/AAAA/CNAME/TXT…)" value={form.type} onChange={(v) => setForm({ ...form, type: v })} />
            <Input label="名称" value={form.name} onChange={(v) => setForm({ ...form, name: v })} />
            <Input label="内容" value={form.content} onChange={(v) => setForm({ ...form, content: v })} />
            <Input label="TTL (1=自动)" type="number" value={form.ttl} onChange={(v) => setForm({ ...form, ttl: v })} />
          </div>
          <label className="flex items-center gap-2 text-sm mt-3 select-none">
            <input
              type="checkbox"
              checked={form.proxied}
              onChange={(event) => setForm({ ...form, proxied: event.target.checked })}
            />
            Cloudflare 代理(橙云,仅 A/AAAA/CNAME)
          </label>
          <div className="mt-3 flex gap-2">
            <UIButton size="sm" disabled={!configured} onClick={() => void saveRecord()}>
              {form.id ? "更新" : "新增"}
            </UIButton>
            {form.id && (
              <UIButton variant="outline" size="sm" onClick={() => setForm(EMPTY_DNS_FORM)}>
                取消编辑
              </UIButton>
            )}
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>解析记录</CardTitle>
          <CardDescription>{configured ? `共 ${records.length} 条` : "请先配置服务商"}</CardDescription>
        </CardHeader>
        <CardContent>
          {records.length === 0 ? (
            <div className="empty-state text-sm">无记录</div>
          ) : (
            <Table>
              <TableHeader>
                <UITableRow>
                  <TableHead>类型</TableHead>
                  <TableHead>名称</TableHead>
                  <TableHead>内容</TableHead>
                  <TableHead>TTL</TableHead>
                  <TableHead>代理</TableHead>
                  <TableHead className="text-right">操作</TableHead>
                </UITableRow>
              </TableHeader>
              <TableBody>
                {records.map((record) => (
                  <UITableRow key={record.id}>
                    <TableCell className="font-mono text-xs">{record.type}</TableCell>
                    <TableCell className="break-all">{record.name}</TableCell>
                    <TableCell className="font-mono text-xs break-all">{record.content}</TableCell>
                    <TableCell className="tabular-nums">{record.ttl === 1 ? "自动" : record.ttl}</TableCell>
                    <TableCell>{record.proxied ? "是" : "否"}</TableCell>
                    <TableCell className="text-right">
                      <div className="flex justify-end gap-2">
                        <UIButton
                          variant="outline"
                          size="sm"
                          onClick={() =>
                            setForm({
                              id: record.id,
                              type: record.type,
                              name: record.name,
                              content: record.content,
                              ttl: String(record.ttl),
                              proxied: record.proxied
                            })
                          }
                        >
                          编辑
                        </UIButton>
                        <UIButton variant="destructive" size="sm" onClick={() => void deleteRecord(record.id)}>
                          删除
                        </UIButton>
                      </div>
                    </TableCell>
                  </UITableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>
    </section>
  );
}

export function AccessLogPage({ clients }: { clients: Clients }) {
  const [path, setPath] = useState("/var/log/nginx/access.log");
  const [result, setResult] = useState<
    Awaited<ReturnType<Clients["accessLog"]["analyzeAccessLog"]>> | undefined
  >(undefined);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  const analyze = async () => {
    setLoading(true);
    try {
      const response = await clients.accessLog.analyzeAccessLog({ path: path.trim(), topN: 10 });
      setResult(response);
      setError("");
    } catch (err) {
      setError(safeError(err));
    } finally {
      setLoading(false);
    }
  };

  const renderTop = (title: string, items: { key: string; count: bigint }[]) => (
    <Card>
      <CardHeader>
        <CardTitle>{title}</CardTitle>
      </CardHeader>
      <CardContent>
        {items.length === 0 ? (
          <div className="empty-state text-sm">无数据</div>
        ) : (
          <Table>
            <TableBody>
              {items.map((item) => (
                <UITableRow key={item.key}>
                  <TableCell className="font-mono text-xs break-all">{item.key}</TableCell>
                  <TableCell className="text-right tabular-nums">{String(item.count)}</TableCell>
                </UITableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </CardContent>
    </Card>
  );

  return (
    <section className="flex flex-col gap-5">
      <header className="flex flex-col gap-1">
        <h1 className="text-2xl font-semibold tracking-tight m-0">网站访问统计</h1>
        <p className="text-sm text-muted-foreground m-0">
          流式解析 combined 格式访问日志(如 nginx access.log),零外部依赖、有界内存。大日志结果可能为近似。
        </p>
      </header>

      <Card>
        <CardContent className="pt-6">
          <div className="grid gap-3 sm:grid-cols-[1fr_auto] sm:items-end">
            <Input label="日志文件路径" value={path} onChange={setPath} />
            <UIButton size="sm" disabled={loading} onClick={() => void analyze()}>
              <BarChart3 className="size-4" />
              {loading ? "分析中…" : "分析"}
            </UIButton>
          </div>
        </CardContent>
      </Card>

      {error && (
        <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive whitespace-pre-line">
          {error}
        </div>
      )}

      {result && (
        <>
          {result.truncated && (
            <div className="rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-sm text-warning">
              数据量较大,已达去重/扫描上限,UV 与 Top 榜单为近似值。
            </div>
          )}
          <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
            {[
              { label: "请求数 (PV)", value: String(result.totalRequests) },
              { label: "独立访客 (UV)", value: String(result.uniqueVisitors) },
              { label: "总流量", value: formatBytes(result.totalBytes) },
              { label: "爬虫请求", value: String(result.botRequests) }
            ].map((tile) => (
              <Card key={tile.label}>
                <CardContent className="pt-6">
                  <div className="text-sm text-muted-foreground">{tile.label}</div>
                  <div className="text-xl font-semibold tabular-nums mt-1">{tile.value}</div>
                </CardContent>
              </Card>
            ))}
          </div>
          <Card>
            <CardHeader>
              <CardTitle>状态码分布</CardTitle>
            </CardHeader>
            <CardContent>
              <div className="grid grid-cols-2 md:grid-cols-4 gap-3 text-sm">
                <div>
                  <div className="text-muted-foreground">2xx 成功</div>
                  <div className="font-medium tabular-nums">{String(result.status2xx)}</div>
                </div>
                <div>
                  <div className="text-muted-foreground">3xx 跳转</div>
                  <div className="font-medium tabular-nums">{String(result.status3xx)}</div>
                </div>
                <div>
                  <div className="text-muted-foreground">4xx 客户端错误</div>
                  <div className="font-medium tabular-nums">{String(result.status4xx)}</div>
                </div>
                <div>
                  <div className="text-muted-foreground">5xx 服务端错误</div>
                  <div className="font-medium tabular-nums">{String(result.status5xx)}</div>
                </div>
              </div>
              <p className="text-xs text-muted-foreground mt-3 m-0">
                已解析 {String(result.parsedLines)} 行,跳过 {String(result.skippedLines)} 行。
              </p>
            </CardContent>
          </Card>
          <div className="grid gap-4 lg:grid-cols-3">
            {renderTop("热门 URL", result.topPaths)}
            {renderTop("Top 来源 IP", result.topIps)}
            {renderTop("Top User-Agent", result.topUserAgents)}
          </div>
        </>
      )}
    </section>
  );
}

export function ToolboxPage({ clients }: { clients: Clients }) {
  const [info, setInfo] = useState<
    { swapTotal: number; swapUsed: number; timezone: string; rootAvail: number } | undefined
  >(undefined);
  const [swapSize, setSwapSize] = useState("512");
  const [timezone, setTimezone] = useState("Asia/Shanghai");
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");

  const load = useCallback(async () => {
    try {
      const response = await clients.toolbox.getToolbox({});
      setInfo({
        swapTotal: Number(response.swapTotalBytes),
        swapUsed: Number(response.swapUsedBytes),
        timezone: response.timezone,
        rootAvail: Number(response.rootAvailableBytes)
      });
      if (response.timezone) setTimezone(response.timezone);
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  }, [clients]);

  useEffect(() => {
    void load();
  }, [load]);

  const createSwap = async () => {
    try {
      const response = await clients.toolbox.createSwap({ sizeMb: Number(swapSize) || 0 });
      setMessage(response.status?.message || "swap 已创建");
      setError("");
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const applyTimezone = async () => {
    try {
      const response = await clients.toolbox.setTimezone({ timezone: timezone.trim() });
      setMessage(response.status?.message || "时区已设置");
      setError("");
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  return (
    <section className="flex flex-col gap-5">
      <header className="flex items-start justify-between gap-4 flex-wrap">
        <div className="flex flex-col gap-1">
          <h1 className="text-2xl font-semibold tracking-tight m-0">系统工具箱</h1>
          <p className="text-sm text-muted-foreground m-0">
            低配 VPS 常用:加 swap、调时区。系统改动需后端设 RUSTPANEL_TOOLBOX_APPLY=1 才生效。
          </p>
        </div>
        <UIButton variant="outline" size="sm" onClick={() => void load()}>
          <RefreshCw className="size-4" />
          刷新
        </UIButton>
      </header>

      {error && (
        <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive whitespace-pre-line">
          {error}
        </div>
      )}
      {message && !error && (
        <div className="rounded-md border border-success/40 bg-success/10 px-3 py-2 text-sm text-success whitespace-pre-line">
          {message}
        </div>
      )}

      <Card>
        <CardHeader>
          <CardTitle>状态</CardTitle>
        </CardHeader>
        <CardContent>
          {info ? (
            <div className="grid grid-cols-2 md:grid-cols-3 gap-3 text-sm">
              <div>
                <div className="text-muted-foreground">Swap</div>
                <div className="font-medium">
                  {formatBytes(info.swapUsed)} / {formatBytes(info.swapTotal)}
                </div>
              </div>
              <div>
                <div className="text-muted-foreground">时区</div>
                <div className="font-medium">{info.timezone || "-"}</div>
              </div>
              <div>
                <div className="text-muted-foreground">根分区可用</div>
                <div className="font-medium">{formatBytes(info.rootAvail)}</div>
              </div>
            </div>
          ) : (
            <div className="empty-state text-sm">读取中…</div>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>创建 Swap</CardTitle>
          <CardDescription>在根分区建 swapfile 并启用 + 写入 /etc/fstab(64–4096 MB)。</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid gap-3 sm:grid-cols-[1fr_auto] sm:items-end">
            <Input label="大小 (MB)" type="number" value={swapSize} onChange={setSwapSize} />
            <UIButton size="sm" onClick={() => void createSwap()}>
              创建
            </UIButton>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>设置时区</CardTitle>
          <CardDescription>如 Asia/Shanghai / UTC(需 /usr/share/zoneinfo 存在该时区)。</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid gap-3 sm:grid-cols-[1fr_auto] sm:items-end">
            <Input label="时区" value={timezone} onChange={setTimezone} />
            <UIButton size="sm" onClick={() => void applyTimezone()}>
              设置
            </UIButton>
          </div>
        </CardContent>
      </Card>
    </section>
  );
}

const USER_ROLES: Array<{ value: UserRole; label: string }> = [
  { value: UserRole.ADMIN, label: "管理员 (全权)" },
  { value: UserRole.OPERATOR, label: "操作员 (读写,除用户管理)" },
  { value: UserRole.READONLY, label: "只读" }
];

function userRoleLabel(role: UserRole): string {
  return USER_ROLES.find((item) => item.value === role)?.label ?? "未知";
}

type UserForm = { username: string; password: string; role: UserRole; editing: boolean };

const emptyUserForm: UserForm = {
  username: "",
  password: "",
  role: UserRole.OPERATOR,
  editing: false
};

export function UserPage({ clients }: { clients: Clients }) {
  const [users, setUsers] = useState<User[]>([]);
  const [form, setForm] = useState<UserForm>(emptyUserForm);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");

  const load = useCallback(async () => {
    try {
      const response = await clients.user.listUsers({});
      setUsers(response.users);
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  }, [clients]);

  useEffect(() => {
    void load();
  }, [load]);

  const save = async () => {
    if (!form.username.trim()) {
      setError("用户名不能为空");
      return;
    }
    if (!form.editing && !form.password.trim()) {
      setError("新建用户必须设置密码");
      return;
    }
    try {
      await clients.user.upsertUser({
        username: form.username.trim(),
        password: form.password,
        role: form.role
      });
      setMessage(`用户 ${form.username} 已保存`);
      setForm(emptyUserForm);
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const edit = (user: User) => {
    setForm({ username: user.username, password: "", role: user.role, editing: true });
    setError("");
    setMessage("");
  };

  const remove = async (user: User) => {
    try {
      await clients.user.deleteUser({ username: user.username });
      setMessage(`用户 ${user.username} 已删除`);
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  return (
    <section className="flex flex-col gap-5">
      <header className="flex items-start justify-between gap-4 flex-wrap">
        <div className="flex flex-col gap-1">
          <h1 className="text-2xl font-semibold tracking-tight m-0">用户管理</h1>
          <p className="text-sm text-muted-foreground m-0">
            env 管理员(RUSTPANEL_ADMIN_*)始终可登录且为管理员;这里管理附加用户与角色。
            管理员=全权,操作员=读写(不含用户管理),只读=仅查询。
          </p>
        </div>
        <UIButton variant="outline" size="sm" onClick={() => void load()}>
          <RefreshCw className="size-4" />
          刷新
        </UIButton>
      </header>

      {error && (
        <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive whitespace-pre-line">
          {error}
        </div>
      )}
      {message && !error && (
        <div className="rounded-md border border-success/40 bg-success/10 px-3 py-2 text-sm text-success whitespace-pre-line">
          {message}
        </div>
      )}

      <Card>
        <CardHeader>
          <CardTitle>{form.editing ? "编辑用户" : "添加用户"}</CardTitle>
          <CardDescription>密码用 PBKDF2-HMAC-SHA256 加盐存储;编辑时留空表示不改密码。</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid gap-3 sm:grid-cols-[1fr_1fr_1fr_auto] sm:items-end">
            <Input
              label="用户名"
              value={form.username}
              onChange={(username) => setForm((prev) => ({ ...prev, username }))}
            />
            <Input
              label={form.editing ? "密码 (留空不改)" : "密码"}
              type="password"
              value={form.password}
              onChange={(password) => setForm((prev) => ({ ...prev, password }))}
            />
            <div className="grid gap-1">
              <UILabel htmlFor="user-role">角色</UILabel>
              <Select
                value={String(form.role)}
                onValueChange={(value) =>
                  setForm((prev) => ({ ...prev, role: Number(value) as UserRole }))
                }
              >
                <SelectTrigger id="user-role">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {USER_ROLES.map((item) => (
                    <SelectItem key={item.value} value={String(item.value)}>
                      {item.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="flex gap-2">
              {form.editing && (
                <UIButton size="sm" variant="outline" onClick={() => setForm(emptyUserForm)}>
                  取消
                </UIButton>
              )}
              <UIButton size="sm" onClick={() => void save()}>
                <Plus className="size-3.5" />
                {form.editing ? "更新" : "保存"}
              </UIButton>
            </div>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>用户列表</CardTitle>
          <CardDescription>共 {users.length} 个附加用户</CardDescription>
        </CardHeader>
        <CardContent>
          {users.length === 0 ? (
            <div className="empty-state text-sm">尚无附加用户(仅 env 管理员)</div>
          ) : (
            <Table>
              <TableHeader>
                <UITableRow>
                  <TableHead>用户名</TableHead>
                  <TableHead>角色</TableHead>
                  <TableHead className="text-right">操作</TableHead>
                </UITableRow>
              </TableHeader>
              <TableBody>
                {users.map((user) => (
                  <UITableRow key={user.username}>
                    <TableCell className="font-medium">{user.username}</TableCell>
                    <TableCell>
                      <Badge variant={user.role === UserRole.ADMIN ? "info" : "secondary"}>
                        {userRoleLabel(user.role)}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-right">
                      <div className="flex justify-end gap-2">
                        <UIButton size="sm" variant="outline" onClick={() => edit(user)}>
                          编辑
                        </UIButton>
                        <UIButton size="sm" variant="outline" onClick={() => void remove(user)}>
                          <Trash2 className="size-3.5" />
                        </UIButton>
                      </div>
                    </TableCell>
                  </UITableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>
    </section>
  );
}
