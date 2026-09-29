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
import { formatBytes, formatDateTime, safeError } from "../lib/format";
import { type Clients } from "../lib/rpc";
import { Plus, RefreshCw, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useLocale } from "../lib/i18n/locale-provider";
import { tGlobal, type MessageKey, type TFn } from "../lib/i18n/translate";

const BACKUP_TARGET_KINDS: Array<{ value: BackupTargetKind; labelKey: MessageKey }> = [
  { value: BackupTargetKind.LOCAL, labelKey: "backup.backupTargetKindLocal" },
  { value: BackupTargetKind.WEBDAV, labelKey: "backup.backupTargetKindWebdav" },
  { value: BackupTargetKind.S3, labelKey: "backup.backupTargetKindS3" }
];

function backupTargetKindLabel(kind: BackupTargetKind, t: TFn = tGlobal): string {
  const found = BACKUP_TARGET_KINDS.find((item) => item.value === kind);
  return found ? t(found.labelKey) : t("backup.unknown");
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
  const { t, locale } = useLocale();
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
      setError(t("backup.targetNameRequired"));
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
      setMessage(t("backup.targetSaved", { name: targetForm.name }));
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
      setMessage(t("backup.targetDeleted", { name: target.name }));
      if (targetForm.id === target.id) setTargetForm(emptyBackupTargetForm);
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const createBackup = async () => {
    const isDb = backupForm.sourceKind === BackupSourceKind.DATABASE;
    if (isDb ? !backupForm.sourceDsn.trim() : !backupForm.sourcePath.trim()) {
      setError(isDb ? t("backup.dsnRequired") : t("backup.pathRequired"));
      return;
    }
    setMessage(t("backup.creatingBackup"));
    setError("");
    try {
      const response = await clients.backup.createBackup({
        sourcePath: backupForm.sourcePath.trim(),
        name: backupForm.name.trim(),
        targetId: backupForm.targetId,
        sourceKind: backupForm.sourceKind,
        sourceDsn: backupForm.sourceDsn.trim()
      });
      setMessage(response.status?.message || t("backup.backupCreated"));
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
        t("backup.confirmRestore", { name: record.name, path: record.sourcePath })
      )
    ) {
      return;
    }
    setMessage(t("backup.restoring", { name: record.name }));
    setError("");
    try {
      const response = await clients.backup.restoreBackup({ id: record.id, restorePath: "" });
      setMessage(t("backup.restoredTo", { path: response.restoredPath }));
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const removeBackup = async (record: BackupRecord) => {
    try {
      await clients.backup.deleteBackup({ id: record.id });
      setMessage(t("backup.backupDeleted", { name: record.name }));
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  return (
    <section className="flex flex-col gap-5">
      <header className="flex items-start justify-between gap-4 flex-wrap">
        <div className="flex flex-col gap-1">
          <h1 className="text-2xl font-semibold tracking-tight m-0">{t("backup.title")}</h1>
          <p className="text-sm text-muted-foreground m-0">{t("backup.subtitle")}</p>
        </div>
        <UIButton variant="outline" size="sm" onClick={() => void load()}>
          <RefreshCw className="size-4" />
          {t("backup.refresh")}
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
          <CardTitle>{t("backup.targetsCardTitle")}</CardTitle>
          <CardDescription>{t("backup.targetsCardDesc")}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid gap-3 sm:grid-cols-2">
            <Input
              label={t("backup.targetName")}
              value={targetForm.name}
              onChange={(name) => setTargetForm((prev) => ({ ...prev, name }))}
            />
            <div className="grid gap-1">
              <UILabel htmlFor="backup-target-kind">{t("backup.kind")}</UILabel>
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
                      {t(item.labelKey)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <Input
              label={t("backup.addressLabel")}
              value={targetForm.endpoint}
              onChange={(endpoint) => setTargetForm((prev) => ({ ...prev, endpoint }))}
            />
            <Input
              label={t("backup.usernameLabel")}
              value={targetForm.username}
              onChange={(username) => setTargetForm((prev) => ({ ...prev, username }))}
            />
            <Input
              label={editingTarget ? t("backup.passwordLabelEdit") : t("backup.passwordLabelNew")}
              type="password"
              value={targetForm.password}
              onChange={(password) => setTargetForm((prev) => ({ ...prev, password }))}
            />
            {targetForm.kind === BackupTargetKind.S3 && (
              <>
                <Input
                  label={t("backup.s3Region")}
                  value={targetForm.region}
                  onChange={(region) => setTargetForm((prev) => ({ ...prev, region }))}
                />
                <Input
                  label={t("backup.s3Bucket")}
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
              {t("backup.enabled")}
            </label>
            <div className="flex gap-2">
              {editingTarget && (
                <UIButton
                  size="sm"
                  variant="outline"
                  onClick={() => setTargetForm(emptyBackupTargetForm)}
                >
                  {t("backup.cancel")}
                </UIButton>
              )}
              <UIButton size="sm" onClick={() => void saveTarget()}>
                <Plus className="size-3.5" />
                {editingTarget ? t("backup.update") : t("backup.save")}
              </UIButton>
            </div>
          </div>

          {targets.length > 0 && (
            <div className="mt-4">
              <Table>
                <TableHeader>
                  <UITableRow>
                    <TableHead>{t("backup.colName")}</TableHead>
                    <TableHead>{t("backup.colKind")}</TableHead>
                    <TableHead>{t("backup.colAddress")}</TableHead>
                    <TableHead>{t("backup.colStatus")}</TableHead>
                    <TableHead className="text-right">{t("backup.colActions")}</TableHead>
                  </UITableRow>
                </TableHeader>
                <TableBody>
                  {targets.map((target) => (
                    <UITableRow key={target.id}>
                      <TableCell className="font-medium">{target.name}</TableCell>
                      <TableCell>{backupTargetKindLabel(target.kind, t)}</TableCell>
                      <TableCell className="font-mono text-xs max-w-[260px] truncate">
                        {target.endpoint || "-"}
                      </TableCell>
                      <TableCell>
                        <Badge variant={target.enabled ? "success" : "secondary"}>
                          {target.enabled ? t("backup.enabled") : t("backup.disabled")}
                        </Badge>
                      </TableCell>
                      <TableCell className="text-right">
                        <div className="flex justify-end gap-2">
                          <UIButton
                            size="sm"
                            variant="outline"
                            onClick={() => editTarget(target)}
                          >
                            {t("backup.edit")}
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
          <CardTitle>{t("backup.createBackupCardTitle")}</CardTitle>
          <CardDescription>{t("backup.createBackupCardDesc")}</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <div className="flex items-center gap-2">
            <UILabel htmlFor="backup-source-kind">{t("backup.sourceLabel")}</UILabel>
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
                <SelectItem value={String(BackupSourceKind.DIRECTORY)}>{t("backup.sourceDirectory")}</SelectItem>
                <SelectItem value={String(BackupSourceKind.DATABASE)}>{t("backup.sourceDatabase")}</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div className="grid gap-3 sm:grid-cols-[1fr_1fr_1fr_auto] sm:items-end">
            {backupForm.sourceKind === BackupSourceKind.DATABASE ? (
              <Input
                label={t("backup.dsnLabel")}
                value={backupForm.sourceDsn}
                onChange={(sourceDsn) => setBackupForm((prev) => ({ ...prev, sourceDsn }))}
              />
            ) : (
              <Input
                label={t("backup.directoryPathLabel")}
                value={backupForm.sourcePath}
                onChange={(sourcePath) => setBackupForm((prev) => ({ ...prev, sourcePath }))}
              />
            )}
            <Input
              label={t("backup.backupNameLabel")}
              value={backupForm.name}
              onChange={(name) => setBackupForm((prev) => ({ ...prev, name }))}
            />
            <div className="grid gap-1">
              <UILabel htmlFor="backup-target">{t("backup.targetLabel")}</UILabel>
              <Select
                value={backupForm.targetId}
                onValueChange={(targetId) => setBackupForm((prev) => ({ ...prev, targetId }))}
              >
                <SelectTrigger id="backup-target">
                  <SelectValue placeholder={t("backup.localOnly")} />
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
              {t("backup.createBackup")}
            </UIButton>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t("backup.backupPointsTitle")}</CardTitle>
          <CardDescription>{t("backup.countSuffix", { count: records.length })}</CardDescription>
        </CardHeader>
        <CardContent>
          {records.length === 0 ? (
            <div className="empty-state text-sm">{t("backup.noBackups")}</div>
          ) : (
            <Table>
              <TableHeader>
                <UITableRow>
                  <TableHead>{t("backup.colName")}</TableHead>
                  <TableHead>{t("backup.colSource")}</TableHead>
                  <TableHead>{t("backup.colSize")}</TableHead>
                  <TableHead>{t("backup.colOffsite")}</TableHead>
                  <TableHead>{t("backup.colTime")}</TableHead>
                  <TableHead className="text-right">{t("backup.colActions")}</TableHead>
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
                        <Badge variant="success">{t("backup.offsiteUploaded")}</Badge>
                      ) : (
                        <Badge variant="secondary">{t("backup.localOnlyBadge")}</Badge>
                      )}
                    </TableCell>
                    <TableCell className="text-xs text-muted-foreground">
                      {formatDateTime(new Date(Number(record.createdAtSeconds) * 1000), locale)}
                    </TableCell>
                    <TableCell className="text-right">
                      <div className="flex justify-end gap-2">
                        <UIButton
                          size="sm"
                          variant="outline"
                          onClick={() => void restoreBackup(record)}
                        >
                          {t("backup.restore")}
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

const NOTIFICATION_CHANNEL_KINDS: Array<{ value: NotificationChannelKind; labelKey: MessageKey }> = [
  { value: NotificationChannelKind.WEBHOOK, labelKey: "backup.notificationChannelWebhook" },
  { value: NotificationChannelKind.TELEGRAM, labelKey: "backup.notificationChannelTelegram" },
  { value: NotificationChannelKind.DINGTALK, labelKey: "backup.notificationChannelDingtalk" },
  { value: NotificationChannelKind.WECOM, labelKey: "backup.notificationChannelWecom" },
  { value: NotificationChannelKind.BARK, labelKey: "backup.notificationChannelBark" }
];

const NOTIFICATION_EVENT_KEYS: Record<number, MessageKey> = {
  [NotificationEventKind.TEST]: "backup.eventTest",
  [NotificationEventKind.CERT_EXPIRY]: "backup.eventCertExpiry",
  [NotificationEventKind.HIGH_LOAD]: "backup.eventHighLoad",
  [NotificationEventKind.DISK_FULL]: "backup.eventDiskFull",
  [NotificationEventKind.SSH_AUTO_BAN]: "backup.eventSshAutoBan",
  [NotificationEventKind.LOGIN_FAILED]: "backup.eventLoginFailed"
};

function notificationChannelKindLabel(kind: NotificationChannelKind, t: TFn = tGlobal): string {
  const found = NOTIFICATION_CHANNEL_KINDS.find((item) => item.value === kind);
  return found ? t(found.labelKey) : t("backup.unknown");
}

function notificationEventLabel(event: NotificationEventKind, t: TFn = tGlobal): string {
  const key = NOTIFICATION_EVENT_KEYS[event];
  return key ? t(key) : t("backup.eventGeneric");
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
  const { t, locale } = useLocale();
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
      setError(t("backup.channelNameRequired"));
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
      setMessage(t("backup.channelSaved", { name: form.name }));
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
      setMessage(t("backup.channelDeleted", { name: channel.name }));
      if (form.id === channel.id) setForm(emptyNotificationChannelForm);
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const testChannel = async (channel: NotificationChannel) => {
    setMessage(t("backup.testingChannel", { name: channel.name }));
    setError("");
    try {
      const response = await clients.notification.testNotificationChannel({ id: channel.id });
      const failed = response.record?.failedChannels ?? [];
      if (failed.length === 0) {
        setMessage(t("backup.channelTestSuccess", { name: channel.name }));
      } else {
        setMessage("");
        setError(t("backup.channelTestFailed", { channels: failed.join(", ") }));
      }
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const saveSettings = async () => {
    try {
      await clients.notification.updateNotificationSettings({ settings: { rules } });
      setMessage(t("backup.rulesSaved"));
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  return (
    <section className="flex flex-col gap-5">
      <header className="flex items-start justify-between gap-4 flex-wrap">
        <div className="flex flex-col gap-1">
          <h1 className="text-2xl font-semibold tracking-tight m-0">{t("backup.notifyTitle")}</h1>
          <p className="text-sm text-muted-foreground m-0">{t("backup.notifySubtitle")}</p>
        </div>
        <UIButton variant="outline" size="sm" onClick={() => void load()}>
          <RefreshCw className="size-4" />
          {t("backup.refresh")}
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
          <CardTitle>{editing ? t("backup.editChannel") : t("backup.addChannel")}</CardTitle>
          <CardDescription>{t("backup.channelFormDesc")}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid gap-3 sm:grid-cols-2">
            <Input
              label={t("backup.channelName")}
              value={form.name}
              onChange={(name) => setForm((prev) => ({ ...prev, name }))}
            />
            <div className="grid gap-1">
              <UILabel htmlFor="notification-kind">{t("backup.kind")}</UILabel>
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
                      {t(item.labelKey)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <Input
              label={t("backup.targetAddressLabel")}
              value={form.target}
              onChange={(target) => setForm((prev) => ({ ...prev, target }))}
            />
            <Input
              label={editing ? t("backup.secretLabelEdit") : t("backup.secretLabelNew")}
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
              {t("backup.enabled")}
            </label>
            <div className="flex gap-2">
              {editing && (
                <UIButton
                  size="sm"
                  variant="outline"
                  onClick={() => setForm(emptyNotificationChannelForm)}
                >
                  {t("backup.cancel")}
                </UIButton>
              )}
              <UIButton size="sm" onClick={() => void saveChannel()}>
                <Plus className="size-3.5" />
                {editing ? t("backup.update") : t("backup.save")}
              </UIButton>
            </div>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t("backup.channelListTitle")}</CardTitle>
          <CardDescription>{t("backup.channelCountSuffix", { count: channels.length })}</CardDescription>
        </CardHeader>
        <CardContent>
          {channels.length === 0 ? (
            <div className="empty-state text-sm">{t("backup.noChannels")}</div>
          ) : (
            <Table>
              <TableHeader>
                <UITableRow>
                  <TableHead>{t("backup.colName")}</TableHead>
                  <TableHead>{t("backup.colKind")}</TableHead>
                  <TableHead>{t("backup.colTarget")}</TableHead>
                  <TableHead>{t("backup.colStatus")}</TableHead>
                  <TableHead className="text-right">{t("backup.colActions")}</TableHead>
                </UITableRow>
              </TableHeader>
              <TableBody>
                {channels.map((channel) => (
                  <UITableRow key={channel.id}>
                    <TableCell className="font-medium">{channel.name}</TableCell>
                    <TableCell>{notificationChannelKindLabel(channel.kind, t)}</TableCell>
                    <TableCell className="font-mono text-xs max-w-[260px] truncate">
                      {channel.target}
                    </TableCell>
                    <TableCell>
                      <Badge variant={channel.enabled ? "success" : "secondary"}>
                        {channel.enabled ? t("backup.enabled") : t("backup.disabled")}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-right">
                      <div className="flex justify-end gap-2">
                        <UIButton
                          size="sm"
                          variant="outline"
                          onClick={() => void testChannel(channel)}
                        >
                          {t("backup.test")}
                        </UIButton>
                        <UIButton size="sm" variant="outline" onClick={() => editChannel(channel)}>
                          {t("backup.edit")}
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
          <CardTitle>{t("backup.eventRulesTitle")}</CardTitle>
          <CardDescription>{t("backup.eventRulesDesc")}</CardDescription>
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
                  <span className="font-medium">{notificationEventLabel(rule.event, t)}</span>
                </label>
                <div className="w-32">
                  <NumberInput
                    label={t("backup.threshold")}
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
                {t("backup.saveRules")}
              </UIButton>
            </div>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t("backup.historyTitle")}</CardTitle>
          <CardDescription>{t("backup.recentCountSuffix", { count: history.length })}</CardDescription>
        </CardHeader>
        <CardContent>
          {history.length === 0 ? (
            <div className="empty-state text-sm">{t("backup.noHistory")}</div>
          ) : (
            <Table>
              <TableHeader>
                <UITableRow>
                  <TableHead>{t("backup.colTime")}</TableHead>
                  <TableHead>{t("backup.colEvent")}</TableHead>
                  <TableHead>{t("backup.colTitle")}</TableHead>
                  <TableHead>{t("backup.colResult")}</TableHead>
                </UITableRow>
              </TableHeader>
              <TableBody>
                {history.map((record) => (
                  <UITableRow key={record.id}>
                    <TableCell className="text-xs text-muted-foreground">
                      {formatDateTime(new Date(Number(record.occurredAtSeconds) * 1000), locale)}
                    </TableCell>
                    <TableCell>{notificationEventLabel(record.event, t)}</TableCell>
                    <TableCell className="max-w-[240px] truncate">{record.title}</TableCell>
                    <TableCell className="text-xs">
                      <span className="text-success">
                        {t("backup.succeededSuffix", { count: record.deliveredChannels.length })}
                      </span>
                      {record.failedChannels.length > 0 && (
                        <span className="text-destructive">
                          {" "}
                          · {t("backup.failedSuffix", { count: record.failedChannels.length })}
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
  const { t } = useLocale();
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
      setError(t("backup.aliasForwardRequired"));
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
      setMessage(t("backup.aliasSaved", { alias: form.alias }));
      setForm({ alias: "", forwardTo: "", note: "" });
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const remove = async (alias: string) => {
    try {
      await clients.vsmtpAlias.deleteVsmtpAlias({ alias });
      setMessage(t("backup.aliasDeleted", { alias }));
      void load();
    } catch (err) {
      setError(safeError(err));
    }
  };

  return (
    <section className="flex flex-col gap-5">
      <header className="flex items-start justify-between gap-4 flex-wrap">
        <div className="flex flex-col gap-1">
          <h1 className="text-2xl font-semibold tracking-tight m-0">{t("backup.vsmtpTitle")}</h1>
          <p className="text-sm text-muted-foreground m-0">{t("backup.vsmtpSubtitle")}</p>
        </div>
        <UIButton variant="outline" size="sm" onClick={() => void load()}>
          <RefreshCw className="size-4" />
          {t("backup.refresh")}
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
          <CardTitle>{t("backup.addUpdateAliasTitle")}</CardTitle>
          <CardDescription>{t("backup.addUpdateAliasDesc")}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid gap-3 sm:grid-cols-[1fr_1fr_1fr_auto] sm:items-end">
            <Input
              label={t("backup.aliasLabel")}
              value={form.alias}
              onChange={(alias) => setForm((prev) => ({ ...prev, alias }))}
            />
            <Input
              label="forward_to"
              value={form.forwardTo}
              onChange={(forwardTo) => setForm((prev) => ({ ...prev, forwardTo }))}
            />
            <Input
              label={t("backup.noteLabel")}
              value={form.note}
              onChange={(note) => setForm((prev) => ({ ...prev, note }))}
            />
            <UIButton size="sm" onClick={() => void upsert()}>
              <Plus className="size-3.5" />
              {t("backup.save")}
            </UIButton>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t("backup.existingAliasTitle")}</CardTitle>
          <CardDescription>{t("backup.countSuffix", { count: aliases.length })}</CardDescription>
        </CardHeader>
        <CardContent>
          {aliases.length === 0 ? (
            <div className="empty-state text-sm">{t("backup.noAliases")}</div>
          ) : (
            <Table>
              <TableHeader>
                <UITableRow>
                  <TableHead>{t("backup.colAlias")}</TableHead>
                  <TableHead>{t("backup.colForwardTo")}</TableHead>
                  <TableHead>{t("backup.colNote")}</TableHead>
                  <TableHead className="text-right">{t("backup.colActions")}</TableHead>
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
                        {t("backup.delete")}
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
