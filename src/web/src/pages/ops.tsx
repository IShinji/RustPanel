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
import { useLocale } from "../lib/i18n/locale-provider";
import { tGlobal, type MessageKey, type TFn } from "../lib/i18n/translate";

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
  const { t } = useLocale();
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
      setMessage(t("ops.dnsConfigSaved"));
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
      setMessage(t("ops.dnsRecordSaved"));
      setError("");
      void loadRecords();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const deleteRecord = async (id: string) => {
    if (!window.confirm(t("ops.dnsConfirmDeleteRecord"))) return;
    try {
      await clients.dns.deleteDnsRecord({ id });
      setMessage(t("ops.dnsRecordDeleted"));
      setError("");
      void loadRecords();
    } catch (err) {
      setError(safeError(err));
    }
  };

  return (
    <section className="flex flex-col gap-5">
      <header className="flex flex-col gap-1">
        <h1 className="text-2xl font-semibold tracking-tight m-0">{t("ops.dnsTitle")}</h1>
        <p className="text-sm text-muted-foreground m-0">{t("ops.dnsSubtitle")}</p>
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
          <CardTitle>{t("ops.dnsProviderConfig")}</CardTitle>
          <CardDescription>
            Cloudflare · {configured ? t("ops.dnsConfigured") : t("ops.dnsNotConfigured")}
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
          <p className="mt-3 text-xs text-muted-foreground">{t("ops.dnsHelpText")}</p>
          <div className="mt-3">
            <UIButton size="sm" onClick={() => void saveConfig()}>
              {t("ops.dnsSaveConfig")}
            </UIButton>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{form.id ? t("ops.dnsEditRecord") : t("ops.dnsNewRecord")}</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
            <Input label={t("ops.dnsType")} value={form.type} onChange={(v) => setForm({ ...form, type: v })} />
            <Input label={t("ops.dnsName")} value={form.name} onChange={(v) => setForm({ ...form, name: v })} />
            <Input label={t("ops.dnsContent")} value={form.content} onChange={(v) => setForm({ ...form, content: v })} />
            <Input label={t("ops.dnsTtl")} type="number" value={form.ttl} onChange={(v) => setForm({ ...form, ttl: v })} />
          </div>
          <label className="flex items-center gap-2 text-sm mt-3 select-none">
            <input
              type="checkbox"
              checked={form.proxied}
              onChange={(event) => setForm({ ...form, proxied: event.target.checked })}
            />
            {t("ops.dnsProxied")}
          </label>
          <div className="mt-3 flex gap-2">
            <UIButton size="sm" disabled={!configured} onClick={() => void saveRecord()}>
              {form.id ? t("ops.update") : t("ops.dnsNew")}
            </UIButton>
            {form.id && (
              <UIButton variant="outline" size="sm" onClick={() => setForm(EMPTY_DNS_FORM)}>
                {t("ops.dnsCancelEdit")}
              </UIButton>
            )}
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t("ops.dnsRecords")}</CardTitle>
          <CardDescription>
            {configured ? t("ops.dnsRecordCount", { count: records.length }) : t("ops.dnsConfigureProviderFirst")}
          </CardDescription>
        </CardHeader>
        <CardContent>
          {records.length === 0 ? (
            <div className="empty-state text-sm">{t("ops.dnsNoRecords")}</div>
          ) : (
            <Table>
              <TableHeader>
                <UITableRow>
                  <TableHead>{t("ops.dnsColType")}</TableHead>
                  <TableHead>{t("ops.dnsColName")}</TableHead>
                  <TableHead>{t("ops.dnsColContent")}</TableHead>
                  <TableHead>{t("ops.dnsColTtl")}</TableHead>
                  <TableHead>{t("ops.dnsColProxied")}</TableHead>
                  <TableHead className="text-right">{t("ops.actions")}</TableHead>
                </UITableRow>
              </TableHeader>
              <TableBody>
                {records.map((record) => (
                  <UITableRow key={record.id}>
                    <TableCell className="font-mono text-xs">{record.type}</TableCell>
                    <TableCell className="break-all">{record.name}</TableCell>
                    <TableCell className="font-mono text-xs break-all">{record.content}</TableCell>
                    <TableCell className="tabular-nums">{record.ttl === 1 ? t("ops.dnsAutoTtl") : record.ttl}</TableCell>
                    <TableCell>{record.proxied ? t("ops.yes") : t("ops.no")}</TableCell>
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
                          {t("ops.edit")}
                        </UIButton>
                        <UIButton variant="destructive" size="sm" onClick={() => void deleteRecord(record.id)}>
                          {t("ops.delete")}
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
  const { t } = useLocale();
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
          <div className="empty-state text-sm">{t("ops.noData")}</div>
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
        <h1 className="text-2xl font-semibold tracking-tight m-0">{t("ops.accessLogTitle")}</h1>
        <p className="text-sm text-muted-foreground m-0">{t("ops.accessLogSubtitle")}</p>
      </header>

      <Card>
        <CardContent className="pt-6">
          <div className="grid gap-3 sm:grid-cols-[1fr_auto] sm:items-end">
            <Input label={t("ops.accessLogPath")} value={path} onChange={setPath} />
            <UIButton size="sm" disabled={loading} onClick={() => void analyze()}>
              <BarChart3 className="size-4" />
              {loading ? t("ops.analyzing") : t("ops.analyze")}
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
              {t("ops.accessLogApproxWarning")}
            </div>
          )}
          <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
            {[
              { label: t("ops.pv"), value: String(result.totalRequests) },
              { label: t("ops.uv"), value: String(result.uniqueVisitors) },
              { label: t("ops.totalTraffic"), value: formatBytes(result.totalBytes) },
              { label: t("ops.botRequests"), value: String(result.botRequests) }
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
              <CardTitle>{t("ops.statusDistribution")}</CardTitle>
            </CardHeader>
            <CardContent>
              <div className="grid grid-cols-2 md:grid-cols-4 gap-3 text-sm">
                <div>
                  <div className="text-muted-foreground">{t("ops.status2xx")}</div>
                  <div className="font-medium tabular-nums">{String(result.status2xx)}</div>
                </div>
                <div>
                  <div className="text-muted-foreground">{t("ops.status3xx")}</div>
                  <div className="font-medium tabular-nums">{String(result.status3xx)}</div>
                </div>
                <div>
                  <div className="text-muted-foreground">{t("ops.status4xx")}</div>
                  <div className="font-medium tabular-nums">{String(result.status4xx)}</div>
                </div>
                <div>
                  <div className="text-muted-foreground">{t("ops.status5xx")}</div>
                  <div className="font-medium tabular-nums">{String(result.status5xx)}</div>
                </div>
              </div>
              <p className="text-xs text-muted-foreground mt-3 m-0">
                {t("ops.parsedSkipped", { parsed: String(result.parsedLines), skipped: String(result.skippedLines) })}
              </p>
            </CardContent>
          </Card>
          <div className="grid gap-4 lg:grid-cols-3">
            {renderTop(t("ops.topUrls"), result.topPaths)}
            {renderTop(t("ops.topSourceIps"), result.topIps)}
            {renderTop(t("ops.topUserAgents"), result.topUserAgents)}
          </div>
        </>
      )}
    </section>
  );
}

export function ToolboxPage({ clients }: { clients: Clients }) {
  const { t } = useLocale();
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
      setMessage(response.status?.message || t("ops.swapCreated"));
      setError("");
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const applyTimezone = async () => {
    try {
      const response = await clients.toolbox.setTimezone({ timezone: timezone.trim() });
      setMessage(response.status?.message || t("ops.timezoneSet"));
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
          <h1 className="text-2xl font-semibold tracking-tight m-0">{t("ops.toolboxTitle")}</h1>
          <p className="text-sm text-muted-foreground m-0">{t("ops.toolboxSubtitle")}</p>
        </div>
        <UIButton variant="outline" size="sm" onClick={() => void load()}>
          <RefreshCw className="size-4" />
          {t("ops.refresh")}
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
          <CardTitle>{t("ops.status")}</CardTitle>
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
                <div className="text-muted-foreground">{t("ops.timezone")}</div>
                <div className="font-medium">{info.timezone || "-"}</div>
              </div>
              <div>
                <div className="text-muted-foreground">{t("ops.rootAvailable")}</div>
                <div className="font-medium">{formatBytes(info.rootAvail)}</div>
              </div>
            </div>
          ) : (
            <div className="empty-state text-sm">{t("ops.loading")}</div>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t("ops.createSwap")}</CardTitle>
          <CardDescription>{t("ops.createSwapDesc")}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid gap-3 sm:grid-cols-[1fr_auto] sm:items-end">
            <Input label={t("ops.sizeMb")} type="number" value={swapSize} onChange={setSwapSize} />
            <UIButton size="sm" onClick={() => void createSwap()}>
              {t("ops.create")}
            </UIButton>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t("ops.setTimezone")}</CardTitle>
          <CardDescription>{t("ops.setTimezoneDesc")}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid gap-3 sm:grid-cols-[1fr_auto] sm:items-end">
            <Input label={t("ops.timezone")} value={timezone} onChange={setTimezone} />
            <UIButton size="sm" onClick={() => void applyTimezone()}>
              {t("ops.set")}
            </UIButton>
          </div>
        </CardContent>
      </Card>
    </section>
  );
}

const USER_ROLES: Array<{ value: UserRole; labelKey: MessageKey }> = [
  { value: UserRole.ADMIN, labelKey: "ops.roleAdmin" },
  { value: UserRole.OPERATOR, labelKey: "ops.roleOperator" },
  { value: UserRole.READONLY, labelKey: "ops.roleReadonly" }
];

function userRoleLabel(role: UserRole, t: TFn = tGlobal): string {
  const found = USER_ROLES.find((item) => item.value === role);
  return found ? t(found.labelKey) : t("ops.unknown");
}

type UserForm = { username: string; password: string; role: UserRole; editing: boolean };

const emptyUserForm: UserForm = {
  username: "",
  password: "",
  role: UserRole.OPERATOR,
  editing: false
};

export function UserPage({ clients }: { clients: Clients }) {
  const { t } = useLocale();
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
      setError(t("ops.usernameRequired"));
      return;
    }
    if (!form.editing && !form.password.trim()) {
      setError(t("ops.passwordRequiredForNewUser"));
      return;
    }
    try {
      await clients.user.upsertUser({
        username: form.username.trim(),
        password: form.password,
        role: form.role
      });
      setMessage(t("ops.userSaved", { username: form.username }));
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
      setMessage(t("ops.userDeleted", { username: user.username }));
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  return (
    <section className="flex flex-col gap-5">
      <header className="flex items-start justify-between gap-4 flex-wrap">
        <div className="flex flex-col gap-1">
          <h1 className="text-2xl font-semibold tracking-tight m-0">{t("ops.userTitle")}</h1>
          <p className="text-sm text-muted-foreground m-0">{t("ops.userSubtitle")}</p>
        </div>
        <UIButton variant="outline" size="sm" onClick={() => void load()}>
          <RefreshCw className="size-4" />
          {t("ops.refresh")}
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
          <CardTitle>{form.editing ? t("ops.editUser") : t("ops.addUser")}</CardTitle>
          <CardDescription>{t("ops.passwordHashDesc")}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid gap-3 sm:grid-cols-[1fr_1fr_1fr_auto] sm:items-end">
            <Input
              label={t("ops.username")}
              value={form.username}
              onChange={(username) => setForm((prev) => ({ ...prev, username }))}
            />
            <Input
              label={form.editing ? t("ops.passwordKeepBlank") : t("ops.password")}
              type="password"
              value={form.password}
              onChange={(password) => setForm((prev) => ({ ...prev, password }))}
            />
            <div className="grid gap-1">
              <UILabel htmlFor="user-role">{t("ops.role")}</UILabel>
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
                      {t(item.labelKey)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="flex gap-2">
              {form.editing && (
                <UIButton size="sm" variant="outline" onClick={() => setForm(emptyUserForm)}>
                  {t("ops.cancel")}
                </UIButton>
              )}
              <UIButton size="sm" onClick={() => void save()}>
                <Plus className="size-3.5" />
                {form.editing ? t("ops.update") : t("ops.save")}
              </UIButton>
            </div>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t("ops.userList")}</CardTitle>
          <CardDescription>{t("ops.additionalUserCount", { count: users.length })}</CardDescription>
        </CardHeader>
        <CardContent>
          {users.length === 0 ? (
            <div className="empty-state text-sm">{t("ops.noAdditionalUsers")}</div>
          ) : (
            <Table>
              <TableHeader>
                <UITableRow>
                  <TableHead>{t("ops.username")}</TableHead>
                  <TableHead>{t("ops.role")}</TableHead>
                  <TableHead className="text-right">{t("ops.actions")}</TableHead>
                </UITableRow>
              </TableHeader>
              <TableBody>
                {users.map((user) => (
                  <UITableRow key={user.username}>
                    <TableCell className="font-medium">{user.username}</TableCell>
                    <TableCell>
                      <Badge variant={user.role === UserRole.ADMIN ? "info" : "secondary"}>
                        {userRoleLabel(user.role, t)}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-right">
                      <div className="flex justify-end gap-2">
                        <UIButton size="sm" variant="outline" onClick={() => edit(user)}>
                          {t("ops.edit")}
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
