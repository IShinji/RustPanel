import { Input, NumberInput } from "../components/form-controls";
import { Badge } from "../components/ui/badge";
import { Button as UIButton } from "../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../components/ui/card";
import { Label as UILabel } from "../components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../components/ui/select";
import { Switch } from "../components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow as UITableRow } from "../components/ui/table";
import { BackupRecord, BackupSourceKind, BackupTarget, BackupTargetKind } from "../gen/rustpanel/v1/backup_pb";
import { NotificationChannel, NotificationChannelKind, NotificationEventKind, NotificationRecord, NotificationRule } from "../gen/rustpanel/v1/notification_pb";
import { VsmtpAlias } from "../gen/rustpanel/v1/vsmtp_pb";
import { formatBytes, safeError } from "../lib/format";
import { type Clients } from "../lib/rpc";
import { Plus, RefreshCw, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

const BACKUP_TARGET_KINDS: Array<{ value: BackupTargetKind; label: string }> = [
  { value: BackupTargetKind.LOCAL, label: "本地" },
  { value: BackupTargetKind.WEBDAV, label: "WebDAV (离站)" },
  { value: BackupTargetKind.S3, label: "S3 兼容 (R2/MinIO/OSS/COS)" }
];

function backupTargetKindLabel(kind: BackupTargetKind): string {
  return BACKUP_TARGET_KINDS.find((item) => item.value === kind)?.label ?? "未知";
}

type BackupTargetForm = {
  id: string;
  name: string;
  kind: BackupTargetKind;
  endpoint: string;
  username: string;
  password: string;
  enabled: boolean;
  region: string;
  bucket: string;
};

const emptyBackupTargetForm: BackupTargetForm = {
  id: "",
  name: "",
  kind: BackupTargetKind.WEBDAV,
  endpoint: "",
  username: "",
  password: "",
  enabled: true,
  region: "",
  bucket: ""
};

export function BackupPage({ clients }: { clients: Clients }) {
  const [targets, setTargets] = useState<BackupTarget[]>([]);
  const [records, setRecords] = useState<BackupRecord[]>([]);
  const [targetForm, setTargetForm] = useState<BackupTargetForm>(emptyBackupTargetForm);
  const [backupForm, setBackupForm] = useState({
    sourcePath: "",
    name: "",
    targetId: "",
    sourceKind: BackupSourceKind.DIRECTORY,
    sourceDsn: ""
  });
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");

  const load = useCallback(async () => {
    try {
      const [targetsResponse, recordsResponse] = await Promise.all([
        clients.backup.listBackupTargets({}),
        clients.backup.listBackups({})
      ]);
      setTargets(targetsResponse.targets);
      setRecords(recordsResponse.records);
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  }, [clients]);

  useEffect(() => {
    void load();
  }, [load]);

  const editingTarget = targetForm.id !== "";

  const saveTarget = async () => {
    if (!targetForm.name.trim()) {
      setError("去向名称不能为空");
      return;
    }
    try {
      await clients.backup.upsertBackupTarget({
        target: {
          id: targetForm.id,
          name: targetForm.name.trim(),
          kind: targetForm.kind,
          endpoint: targetForm.endpoint.trim(),
          username: targetForm.username.trim(),
          password: targetForm.password,
          enabled: targetForm.enabled,
          createdAtSeconds: 0n,
          region: targetForm.region.trim(),
          bucket: targetForm.bucket.trim()
        }
      });
      setMessage(`去向 ${targetForm.name} 已保存`);
      setTargetForm(emptyBackupTargetForm);
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const editTarget = (target: BackupTarget) => {
    setTargetForm({
      id: target.id,
      name: target.name,
      kind: target.kind,
      endpoint: target.endpoint,
      username: target.username,
      password: "",
      enabled: target.enabled,
      region: target.region,
      bucket: target.bucket
    });
    setError("");
    setMessage("");
  };

  const removeTarget = async (target: BackupTarget) => {
    try {
      await clients.backup.deleteBackupTarget({ id: target.id });
      setMessage(`去向 ${target.name} 已删除`);
      if (targetForm.id === target.id) setTargetForm(emptyBackupTargetForm);
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const createBackup = async () => {
    const isDb = backupForm.sourceKind === BackupSourceKind.DATABASE;
    if (isDb ? !backupForm.sourceDsn.trim() : !backupForm.sourcePath.trim()) {
      setError(isDb ? "请填写数据库 DSN" : "请填写要备份的目录绝对路径");
      return;
    }
    setMessage("正在创建备份...");
    setError("");
    try {
      const response = await clients.backup.createBackup({
        sourcePath: backupForm.sourcePath.trim(),
        name: backupForm.name.trim(),
        targetId: backupForm.targetId,
        sourceKind: backupForm.sourceKind,
        sourceDsn: backupForm.sourceDsn.trim()
      });
      setMessage(response.status?.message || "备份已创建");
      setBackupForm({
        sourcePath: "",
        name: "",
        targetId: "",
        sourceKind: BackupSourceKind.DIRECTORY,
        sourceDsn: ""
      });
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const restoreBackup = async (record: BackupRecord) => {
    if (
      !window.confirm(
        `确定把备份「${record.name}」还原到 ${record.sourcePath}?该目录现有内容会被覆盖。`
      )
    ) {
      return;
    }
    setMessage(`正在还原 ${record.name}...`);
    setError("");
    try {
      const response = await clients.backup.restoreBackup({ id: record.id, restorePath: "" });
      setMessage(`已还原到 ${response.restoredPath}`);
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const removeBackup = async (record: BackupRecord) => {
    try {
      await clients.backup.deleteBackup({ id: record.id });
      setMessage(`备份 ${record.name} 已删除`);
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  return (
    <section className="flex flex-col gap-5">
      <header className="flex items-start justify-between gap-4 flex-wrap">
        <div className="flex flex-col gap-1">
          <h1 className="text-2xl font-semibold tracking-tight m-0">备份与还原</h1>
          <p className="text-sm text-muted-foreground m-0">
            把目录打成 tar.gz 归档(始终先落本地),WebDAV 去向额外推一份离站副本;支持一键还原。
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
          <CardTitle>备份去向</CardTitle>
          <CardDescription>
            本地无需配置;WebDAV 填目录完整 URL + 账号密码;S3 填 endpoint / 区域 / 桶 / AK / SK
            (兼容 R2 / MinIO / OSS / COS,路径风格)。
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid gap-3 sm:grid-cols-2">
            <Input
              label="去向名称"
              value={targetForm.name}
              onChange={(name) => setTargetForm((prev) => ({ ...prev, name }))}
            />
            <div className="grid gap-1">
              <UILabel htmlFor="backup-target-kind">类型</UILabel>
              <Select
                value={String(targetForm.kind)}
                onValueChange={(value) =>
                  setTargetForm((prev) => ({ ...prev, kind: Number(value) as BackupTargetKind }))
                }
              >
                <SelectTrigger id="backup-target-kind">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {BACKUP_TARGET_KINDS.map((item) => (
                    <SelectItem key={item.value} value={String(item.value)}>
                      {item.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <Input
              label="地址 (WebDAV 目录 URL / S3 endpoint)"
              value={targetForm.endpoint}
              onChange={(endpoint) => setTargetForm((prev) => ({ ...prev, endpoint }))}
            />
            <Input
              label="用户名 / S3 Access Key"
              value={targetForm.username}
              onChange={(username) => setTargetForm((prev) => ({ ...prev, username }))}
            />
            <Input
              label={editingTarget ? "密码 / Secret Key (留空保持不变)" : "密码 / S3 Secret Key"}
              type="password"
              value={targetForm.password}
              onChange={(password) => setTargetForm((prev) => ({ ...prev, password }))}
            />
            {targetForm.kind === BackupTargetKind.S3 && (
              <>
                <Input
                  label="S3 区域 (region,如 us-east-1 / auto)"
                  value={targetForm.region}
                  onChange={(region) => setTargetForm((prev) => ({ ...prev, region }))}
                />
                <Input
                  label="S3 桶名 (bucket)"
                  value={targetForm.bucket}
                  onChange={(bucket) => setTargetForm((prev) => ({ ...prev, bucket }))}
                />
              </>
            )}
          </div>
          <div className="mt-3 flex items-center justify-between gap-4">
            <label className="flex items-center gap-2 text-sm">
              <Switch
                checked={targetForm.enabled}
                onCheckedChange={(enabled) => setTargetForm((prev) => ({ ...prev, enabled }))}
              />
              启用
            </label>
            <div className="flex gap-2">
              {editingTarget && (
                <UIButton
                  size="sm"
                  variant="outline"
                  onClick={() => setTargetForm(emptyBackupTargetForm)}
                >
                  取消
                </UIButton>
              )}
              <UIButton size="sm" onClick={() => void saveTarget()}>
                <Plus className="size-3.5" />
                {editingTarget ? "更新" : "保存"}
              </UIButton>
            </div>
          </div>

          {targets.length > 0 && (
            <div className="mt-4">
              <Table>
                <TableHeader>
                  <UITableRow>
                    <TableHead>名称</TableHead>
                    <TableHead>类型</TableHead>
                    <TableHead>地址</TableHead>
                    <TableHead>状态</TableHead>
                    <TableHead className="text-right">操作</TableHead>
                  </UITableRow>
                </TableHeader>
                <TableBody>
                  {targets.map((target) => (
                    <UITableRow key={target.id}>
                      <TableCell className="font-medium">{target.name}</TableCell>
                      <TableCell>{backupTargetKindLabel(target.kind)}</TableCell>
                      <TableCell className="font-mono text-xs max-w-[260px] truncate">
                        {target.endpoint || "-"}
                      </TableCell>
                      <TableCell>
                        <Badge variant={target.enabled ? "success" : "secondary"}>
                          {target.enabled ? "启用" : "停用"}
                        </Badge>
                      </TableCell>
                      <TableCell className="text-right">
                        <div className="flex justify-end gap-2">
                          <UIButton
                            size="sm"
                            variant="outline"
                            onClick={() => editTarget(target)}
                          >
                            编辑
                          </UIButton>
                          <UIButton
                            size="sm"
                            variant="outline"
                            onClick={() => void removeTarget(target)}
                          >
                            <Trash2 className="size-3.5" />
                          </UIButton>
                        </div>
                      </TableCell>
                    </UITableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>创建备份</CardTitle>
          <CardDescription>
            备份目录(如站点 webroot)或数据库(mysqldump/pg_dump,需已安装;SQLite 直接拷文件)。
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <div className="flex items-center gap-2">
            <UILabel htmlFor="backup-source-kind">来源</UILabel>
            <Select
              value={String(backupForm.sourceKind)}
              onValueChange={(value) =>
                setBackupForm((prev) => ({
                  ...prev,
                  sourceKind: Number(value) as BackupSourceKind
                }))
              }
            >
              <SelectTrigger id="backup-source-kind" className="w-40">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={String(BackupSourceKind.DIRECTORY)}>目录</SelectItem>
                <SelectItem value={String(BackupSourceKind.DATABASE)}>数据库</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div className="grid gap-3 sm:grid-cols-[1fr_1fr_1fr_auto] sm:items-end">
            {backupForm.sourceKind === BackupSourceKind.DATABASE ? (
              <Input
                label="数据库 DSN"
                value={backupForm.sourceDsn}
                onChange={(sourceDsn) => setBackupForm((prev) => ({ ...prev, sourceDsn }))}
              />
            ) : (
              <Input
                label="目录绝对路径"
                value={backupForm.sourcePath}
                onChange={(sourcePath) => setBackupForm((prev) => ({ ...prev, sourcePath }))}
              />
            )}
            <Input
              label="备份名(可选)"
              value={backupForm.name}
              onChange={(name) => setBackupForm((prev) => ({ ...prev, name }))}
            />
            <div className="grid gap-1">
              <UILabel htmlFor="backup-target">去向</UILabel>
              <Select
                value={backupForm.targetId}
                onValueChange={(targetId) => setBackupForm((prev) => ({ ...prev, targetId }))}
              >
                <SelectTrigger id="backup-target">
                  <SelectValue placeholder="仅本地" />
                </SelectTrigger>
                <SelectContent>
                  {targets
                    .filter((target) => target.enabled)
                    .map((target) => (
                      <SelectItem key={target.id} value={target.id}>
                        {target.name}
                      </SelectItem>
                    ))}
                </SelectContent>
              </Select>
            </div>
            <UIButton size="sm" onClick={() => void createBackup()}>
              <Plus className="size-3.5" />
              备份
            </UIButton>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>备份点</CardTitle>
          <CardDescription>共 {records.length} 个</CardDescription>
        </CardHeader>
        <CardContent>
          {records.length === 0 ? (
            <div className="empty-state text-sm">暂无备份</div>
          ) : (
            <Table>
              <TableHeader>
                <UITableRow>
                  <TableHead>名称</TableHead>
                  <TableHead>来源</TableHead>
                  <TableHead>大小</TableHead>
                  <TableHead>离站</TableHead>
                  <TableHead>时间</TableHead>
                  <TableHead className="text-right">操作</TableHead>
                </UITableRow>
              </TableHeader>
              <TableBody>
                {records.map((record) => (
                  <UITableRow key={record.id}>
                    <TableCell className="font-medium">{record.name}</TableCell>
                    <TableCell className="font-mono text-xs max-w-[220px] truncate">
                      {record.sourcePath}
                    </TableCell>
                    <TableCell className="text-xs">{formatBytes(Number(record.sizeBytes))}</TableCell>
                    <TableCell>
                      {record.offsiteUploaded ? (
                        <Badge variant="success">已离站</Badge>
                      ) : (
                        <Badge variant="secondary">仅本地</Badge>
                      )}
                    </TableCell>
                    <TableCell className="text-xs text-muted-foreground">
                      {new Date(Number(record.createdAtSeconds) * 1000).toLocaleString()}
                    </TableCell>
                    <TableCell className="text-right">
                      <div className="flex justify-end gap-2">
                        <UIButton
                          size="sm"
                          variant="outline"
                          onClick={() => void restoreBackup(record)}
                        >
                          还原
                        </UIButton>
                        <UIButton
                          size="sm"
                          variant="outline"
                          onClick={() => void removeBackup(record)}
                        >
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

const NOTIFICATION_CHANNEL_KINDS: Array<{ value: NotificationChannelKind; label: string }> = [
  { value: NotificationChannelKind.WEBHOOK, label: "Webhook (通用)" },
  { value: NotificationChannelKind.TELEGRAM, label: "Telegram" },
  { value: NotificationChannelKind.DINGTALK, label: "钉钉" },
  { value: NotificationChannelKind.WECOM, label: "企业微信" },
  { value: NotificationChannelKind.BARK, label: "Bark (iOS)" }
];

const NOTIFICATION_EVENT_LABELS: Record<number, string> = {
  [NotificationEventKind.TEST]: "测试",
  [NotificationEventKind.CERT_EXPIRY]: "证书到期",
  [NotificationEventKind.HIGH_LOAD]: "高负载",
  [NotificationEventKind.DISK_FULL]: "磁盘将满",
  [NotificationEventKind.SSH_AUTO_BAN]: "SSH 自动封禁",
  [NotificationEventKind.LOGIN_FAILED]: "登录失败"
};

function notificationChannelKindLabel(kind: NotificationChannelKind): string {
  return NOTIFICATION_CHANNEL_KINDS.find((item) => item.value === kind)?.label ?? "未知";
}

function notificationEventLabel(event: NotificationEventKind): string {
  return NOTIFICATION_EVENT_LABELS[event] ?? "事件";
}

type NotificationChannelForm = {
  id: string;
  name: string;
  kind: NotificationChannelKind;
  target: string;
  secret: string;
  enabled: boolean;
};

const emptyNotificationChannelForm: NotificationChannelForm = {
  id: "",
  name: "",
  kind: NotificationChannelKind.WEBHOOK,
  target: "",
  secret: "",
  enabled: true
};

export function NotificationPage({ clients }: { clients: Clients }) {
  const [channels, setChannels] = useState<NotificationChannel[]>([]);
  const [rules, setRules] = useState<NotificationRule[]>([]);
  const [history, setHistory] = useState<NotificationRecord[]>([]);
  const [form, setForm] = useState<NotificationChannelForm>(emptyNotificationChannelForm);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");

  const load = useCallback(async () => {
    try {
      const [channelsResponse, settingsResponse, historyResponse] = await Promise.all([
        clients.notification.listNotificationChannels({}),
        clients.notification.getNotificationSettings({}),
        clients.notification.listNotificationHistory({ limit: 50 })
      ]);
      setChannels(channelsResponse.channels);
      setRules(settingsResponse.settings?.rules ?? []);
      setHistory(historyResponse.records);
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  }, [clients]);

  useEffect(() => {
    void load();
  }, [load]);

  const editing = form.id !== "";

  const saveChannel = async () => {
    if (!form.name.trim() || !form.target.trim()) {
      setError("渠道名称与目标地址不能为空");
      return;
    }
    try {
      await clients.notification.upsertNotificationChannel({
        channel: {
          id: form.id,
          name: form.name.trim(),
          kind: form.kind,
          target: form.target.trim(),
          secret: form.secret,
          enabled: form.enabled,
          createdAtSeconds: 0n,
          updatedAtSeconds: 0n
        }
      });
      setMessage(`渠道 ${form.name} 已保存`);
      setForm(emptyNotificationChannelForm);
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const editChannel = (channel: NotificationChannel) => {
    setForm({
      id: channel.id,
      name: channel.name,
      kind: channel.kind,
      target: channel.target,
      // 密钥不回显,留空即"保持不变"(后端识别空 / 脱敏占位)。
      secret: "",
      enabled: channel.enabled
    });
    setError("");
    setMessage("");
  };

  const removeChannel = async (channel: NotificationChannel) => {
    try {
      await clients.notification.deleteNotificationChannel({ id: channel.id });
      setMessage(`渠道 ${channel.name} 已删除`);
      if (form.id === channel.id) setForm(emptyNotificationChannelForm);
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const testChannel = async (channel: NotificationChannel) => {
    setMessage(`正在向 ${channel.name} 发送测试...`);
    setError("");
    try {
      const response = await clients.notification.testNotificationChannel({ id: channel.id });
      const failed = response.record?.failedChannels ?? [];
      if (failed.length === 0) {
        setMessage(`渠道 ${channel.name} 测试发送成功`);
      } else {
        setMessage("");
        setError(`测试发送失败:${failed.join(", ")}`);
      }
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const saveSettings = async () => {
    try {
      await clients.notification.updateNotificationSettings({ settings: { rules } });
      setMessage("通知规则已保存");
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  return (
    <section className="flex flex-col gap-5">
      <header className="flex items-start justify-between gap-4 flex-wrap">
        <div className="flex flex-col gap-1">
          <h1 className="text-2xl font-semibold tracking-tight m-0">通知告警</h1>
          <p className="text-sm text-muted-foreground m-0">
            把证书到期、SSH 自动封禁等事件推送到 Webhook / Telegram / 钉钉 / 企业微信 / Bark。
            密钥保存后不再回显,留空即保持不变。
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
          <CardTitle>{editing ? "编辑渠道" : "添加渠道"}</CardTitle>
          <CardDescription>
            目标地址:Webhook/钉钉/企业微信 填完整 URL,Telegram 填 chat_id,Bark 填
            https://api.day.app/&lt;key&gt;。
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid gap-3 sm:grid-cols-2">
            <Input
              label="渠道名称"
              value={form.name}
              onChange={(name) => setForm((prev) => ({ ...prev, name }))}
            />
            <div className="grid gap-1">
              <UILabel htmlFor="notification-kind">类型</UILabel>
              <Select
                value={String(form.kind)}
                onValueChange={(value) =>
                  setForm((prev) => ({ ...prev, kind: Number(value) as NotificationChannelKind }))
                }
              >
                <SelectTrigger id="notification-kind">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {NOTIFICATION_CHANNEL_KINDS.map((item) => (
                    <SelectItem key={item.value} value={String(item.value)}>
                      {item.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <Input
              label="目标地址 (URL / chat_id)"
              value={form.target}
              onChange={(target) => setForm((prev) => ({ ...prev, target }))}
            />
            <Input
              label={editing ? "密钥 (留空保持不变)" : "密钥 (按需,如 Telegram bot token)"}
              type="password"
              value={form.secret}
              onChange={(secret) => setForm((prev) => ({ ...prev, secret }))}
            />
          </div>
          <div className="mt-3 flex items-center justify-between gap-4">
            <label className="flex items-center gap-2 text-sm">
              <Switch
                checked={form.enabled}
                onCheckedChange={(enabled) => setForm((prev) => ({ ...prev, enabled }))}
              />
              启用
            </label>
            <div className="flex gap-2">
              {editing && (
                <UIButton
                  size="sm"
                  variant="outline"
                  onClick={() => setForm(emptyNotificationChannelForm)}
                >
                  取消
                </UIButton>
              )}
              <UIButton size="sm" onClick={() => void saveChannel()}>
                <Plus className="size-3.5" />
                {editing ? "更新" : "保存"}
              </UIButton>
            </div>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>渠道列表</CardTitle>
          <CardDescription>共 {channels.length} 个渠道</CardDescription>
        </CardHeader>
        <CardContent>
          {channels.length === 0 ? (
            <div className="empty-state text-sm">尚未配置任何通知渠道</div>
          ) : (
            <Table>
              <TableHeader>
                <UITableRow>
                  <TableHead>名称</TableHead>
                  <TableHead>类型</TableHead>
                  <TableHead>目标</TableHead>
                  <TableHead>状态</TableHead>
                  <TableHead className="text-right">操作</TableHead>
                </UITableRow>
              </TableHeader>
              <TableBody>
                {channels.map((channel) => (
                  <UITableRow key={channel.id}>
                    <TableCell className="font-medium">{channel.name}</TableCell>
                    <TableCell>{notificationChannelKindLabel(channel.kind)}</TableCell>
                    <TableCell className="font-mono text-xs max-w-[260px] truncate">
                      {channel.target}
                    </TableCell>
                    <TableCell>
                      <Badge variant={channel.enabled ? "success" : "secondary"}>
                        {channel.enabled ? "启用" : "停用"}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-right">
                      <div className="flex justify-end gap-2">
                        <UIButton
                          size="sm"
                          variant="outline"
                          onClick={() => void testChannel(channel)}
                        >
                          测试
                        </UIButton>
                        <UIButton size="sm" variant="outline" onClick={() => editChannel(channel)}>
                          编辑
                        </UIButton>
                        <UIButton
                          size="sm"
                          variant="outline"
                          onClick={() => void removeChannel(channel)}
                        >
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

      <Card>
        <CardHeader>
          <CardTitle>事件规则</CardTitle>
          <CardDescription>仅启用的事件会触发通知;阈值含义随事件而定(如证书=提前天数)。</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="flex flex-col gap-3">
            {rules.map((rule) => (
              <div
                key={rule.event}
                className="flex items-center justify-between gap-4 rounded-md border border-border bg-card px-4 py-3"
              >
                <label className="flex items-center gap-3 text-sm">
                  <Switch
                    checked={rule.enabled}
                    onCheckedChange={(enabled) =>
                      setRules((prev) =>
                        prev.map((item) =>
                          item.event === rule.event ? { ...item, enabled } : item
                        )
                      )
                    }
                  />
                  <span className="font-medium">{notificationEventLabel(rule.event)}</span>
                </label>
                <div className="w-32">
                  <NumberInput
                    label="阈值"
                    value={rule.threshold}
                    onChange={(threshold) =>
                      setRules((prev) =>
                        prev.map((item) =>
                          item.event === rule.event ? { ...item, threshold } : item
                        )
                      )
                    }
                  />
                </div>
              </div>
            ))}
            <div>
              <UIButton size="sm" onClick={() => void saveSettings()}>
                保存规则
              </UIButton>
            </div>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>发送历史</CardTitle>
          <CardDescription>最近 {history.length} 条</CardDescription>
        </CardHeader>
        <CardContent>
          {history.length === 0 ? (
            <div className="empty-state text-sm">暂无发送记录</div>
          ) : (
            <Table>
              <TableHeader>
                <UITableRow>
                  <TableHead>时间</TableHead>
                  <TableHead>事件</TableHead>
                  <TableHead>标题</TableHead>
                  <TableHead>结果</TableHead>
                </UITableRow>
              </TableHeader>
              <TableBody>
                {history.map((record) => (
                  <UITableRow key={record.id}>
                    <TableCell className="text-xs text-muted-foreground">
                      {new Date(Number(record.occurredAtSeconds) * 1000).toLocaleString()}
                    </TableCell>
                    <TableCell>{notificationEventLabel(record.event)}</TableCell>
                    <TableCell className="max-w-[240px] truncate">{record.title}</TableCell>
                    <TableCell className="text-xs">
                      <span className="text-success">{record.deliveredChannels.length} 成功</span>
                      {record.failedChannels.length > 0 && (
                        <span className="text-destructive">
                          {" "}
                          · {record.failedChannels.length} 失败
                        </span>
                      )}
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

export function VsmtpAliasPage({ clients }: { clients: Clients }) {
  const [aliases, setAliases] = useState<VsmtpAlias[]>([]);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  const [form, setForm] = useState({ alias: "", forwardTo: "", note: "" });

  const load = useCallback(async () => {
    try {
      const response = await clients.vsmtpAlias.listVsmtpAliases({});
      setAliases(response.aliases);
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  }, [clients]);

  useEffect(() => {
    void load();
  }, [load]);

  const upsert = async () => {
    if (!form.alias.trim() || !form.forwardTo.trim()) {
      setError("alias 与转发邮箱都不能为空");
      return;
    }
    try {
      await clients.vsmtpAlias.upsertVsmtpAlias({
        alias: {
          alias: form.alias.trim(),
          forwardTo: form.forwardTo.trim(),
          note: form.note.trim(),
          createdAtSeconds: 0n,
          updatedAtSeconds: 0n
        }
      });
      setMessage(`${form.alias} 已保存`);
      setForm({ alias: "", forwardTo: "", note: "" });
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const remove = async (alias: string) => {
    try {
      await clients.vsmtpAlias.deleteVsmtpAlias({ alias });
      setMessage(`${alias} 已删除`);
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  return (
    <section className="flex flex-col gap-5">
      <header className="flex items-start justify-between gap-4 flex-wrap">
        <div className="flex flex-col gap-1">
          <h1 className="text-2xl font-semibold tracking-tight m-0">邮件别名(vSMTP)</h1>
          <p className="text-sm text-muted-foreground m-0">
            外部发到 alias@你的域名 的邮件会按下方规则转发到 forward_to。
            出站必须在 vSMTP 配置里走 SMTP relay(Resend / SES / Postmark),
            **绝不直连 25 端口**。
          </p>
        </div>
        <UIButton variant="outline" size="sm" onClick={() => void load()}>
          <RefreshCw className="size-4" />
          刷新
        </UIButton>
      </header>

      {error && (
        <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
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
          <CardTitle>添加 / 更新 alias</CardTitle>
          <CardDescription>同名 alias 会被覆盖,创建时间保留</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid gap-3 sm:grid-cols-[1fr_1fr_1fr_auto] sm:items-end">
            <Input
              label="alias(小写 / 数字 / . _ -)"
              value={form.alias}
              onChange={(alias) => setForm((prev) => ({ ...prev, alias }))}
            />
            <Input
              label="forward_to"
              value={form.forwardTo}
              onChange={(forwardTo) => setForm((prev) => ({ ...prev, forwardTo }))}
            />
            <Input
              label="备注(可选)"
              value={form.note}
              onChange={(note) => setForm((prev) => ({ ...prev, note }))}
            />
            <UIButton size="sm" onClick={() => void upsert()}>
              <Plus className="size-3.5" />
              保存
            </UIButton>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>已有 alias</CardTitle>
          <CardDescription>共 {aliases.length} 条</CardDescription>
        </CardHeader>
        <CardContent>
          {aliases.length === 0 ? (
            <div className="empty-state text-sm">尚未配置任何 alias</div>
          ) : (
            <Table>
              <TableHeader>
                <UITableRow>
                  <TableHead>alias</TableHead>
                  <TableHead>转发到</TableHead>
                  <TableHead>备注</TableHead>
                  <TableHead className="text-right">操作</TableHead>
                </UITableRow>
              </TableHeader>
              <TableBody>
                {aliases.map((item) => (
                  <UITableRow key={item.alias}>
                    <TableCell className="font-mono text-xs">{item.alias}</TableCell>
                    <TableCell className="font-mono text-xs">{item.forwardTo}</TableCell>
                    <TableCell className="text-muted-foreground">
                      {item.note || "-"}
                    </TableCell>
                    <TableCell className="text-right">
                      <UIButton
                        size="sm"
                        variant="outline"
                        onClick={() => void remove(item.alias)}
                      >
                        <Trash2 className="size-3.5" />
                        删除
                      </UIButton>
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
