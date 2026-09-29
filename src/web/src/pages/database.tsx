import { CodeEditor as Editor } from "../components/code-editor";
import { IconButton, Input, StatusPill } from "../components/form-controls";
import { Button as UIButton } from "../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../components/ui/card";
import { Input as UIInput } from "../components/ui/input";
import { Label as UILabel } from "../components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../components/ui/select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow as UITableRow } from "../components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../components/ui/tabs";
import { CronRunState, CronTask, CronTaskState } from "../gen/rustpanel/v1/cron_pb";
import { RedisInfo, SqliteFile } from "../gen/rustpanel/v1/db_pb";
import { formatBytes, formatDateTime, formatDuration, safeError } from "../lib/format";
import { type Clients } from "../lib/rpc";
import { Clock, Download, FileText, Pause, Pencil, Play, Plus, RefreshCw, RotateCw, Save, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useMountEffect } from "../lib/hooks";
import { useLocale } from "../lib/i18n/locale-provider";
import { type MessageKey } from "../lib/i18n/translate";

function downloadCsv(columns: string[], rows: string[][], filename: string) {
  const escape = (value: string) => `"${String(value ?? "").replace(/"/g, '""')}"`;
  const lines = [columns.map(escape).join(","), ...rows.map((row) => row.map(escape).join(","))];
  const blob = new Blob(["﻿" + lines.join("\r\n")], { type: "text/csv;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  anchor.click();
  URL.revokeObjectURL(url);
}

export function DatabasePanel({ clients }: { clients: Clients }) {
  const { t, locale } = useLocale();
  const [dsn, setDsn] = useState("sqlite::memory:");
  const [sql, setSql] = useState("select 1 as value");
  const [columns, setColumns] = useState<string[]>([]);
  const [rows, setRows] = useState<string[][]>([]);
  const [databases, setDatabases] = useState<string[]>([]);
  const [newDbName, setNewDbName] = useState("");
  const [userForm, setUserForm] = useState({
    username: "app",
    password: "",
    database: ""
  });
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  // ====== Phase D: SQLite 文件管理 ======
  const [sqliteFiles, setSqliteFiles] = useState<SqliteFile[]>([]);
  const [scanDirs, setScanDirs] = useState("");
  const [newSqlitePath, setNewSqlitePath] = useState("");
  // ====== Phase D: Redis 监控 ======
  const [redisUrl, setRedisUrl] = useState("redis://127.0.0.1:6379");
  const [redisInfo, setRedisInfo] = useState<RedisInfo | undefined>(undefined);
  // ====== P2: 表浏览 ======
  const [tables, setTables] = useState<string[]>([]);
  const [browseName, setBrowseName] = useState("");
  const [browseColumns, setBrowseColumns] = useState<string[]>([]);
  const [browseRows, setBrowseRows] = useState<string[][]>([]);
  const [browseTotal, setBrowseTotal] = useState(0);
  const [browseOffset, setBrowseOffset] = useState(0);
  const [overview, setOverview] = useState<
    { version: string; connections: number; uptime: number } | undefined
  >(undefined);

  const refreshSqlite = useCallback(async () => {
    try {
      const response = await clients.database.listSqliteFiles({
        scanDirs: scanDirs
          .split(/[\s,]+/)
          .map((s) => s.trim())
          .filter(Boolean)
      });
      setSqliteFiles(response.files);
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  }, [clients, scanDirs]);

  const createSqliteFile = async () => {
    if (!newSqlitePath.trim()) {
      setError(t("database.sqlitePathRequired"));
      return;
    }
    try {
      await clients.database.createSqliteFile({ path: newSqlitePath.trim() });
      setMessage(t("database.sqliteCreated", { path: newSqlitePath }));
      setNewSqlitePath("");
      void refreshSqlite();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const vacuumSqliteFile = async (path: string) => {
    try {
      const response = await clients.database.vacuumSqlite({ path });
      const saved =
        Number(response.sizeBeforeBytes) - Number(response.sizeAfterBytes);
      setMessage(
        saved > 0
          ? t("database.vacuumDoneSaved", { size: formatBytes(BigInt(saved)) })
          : t("database.vacuumDoneNoSpace")
      );
      void refreshSqlite();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const refreshRedis = useCallback(async () => {
    try {
      const response = await clients.database.getRedisInfo({ url: redisUrl });
      setRedisInfo(response.info);
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  }, [clients, redisUrl]);

  useEffect(() => {
    void refreshSqlite();
    void refreshRedis();
  }, [refreshSqlite, refreshRedis]);

  const listDatabases = useCallback(async () => {
    try {
      const response = await clients.database.listDatabases({ dsn });
      setDatabases(response.databases.map((database) => database.name));
      setMessage(t("database.connectedCount", { count: response.databases.length }));
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  }, [clients, dsn, t]);

  const createDatabase = async () => {
    if (!newDbName.trim()) return;
    try {
      await clients.database.createDatabase({ dsn, name: newDbName.trim() });
      setMessage(t("database.databaseCreated", { name: newDbName }));
      setNewDbName("");
      void listDatabases();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const backupDatabase = async (database: string) => {
    try {
      const response = await clients.database.backupDatabase({ dsn, database });
      setMessage(t("database.backupDone", { url: response.downloadUrl || t("database.backupLinkGenerated") }));
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  };

  const createDatabaseUser = async () => {
    try {
      await clients.database.createDatabaseUser({
        dsn,
        username: userForm.username,
        password: userForm.password,
        database: userForm.database
      });
      setMessage(t("database.userCreated", { username: userForm.username, database: userForm.database }));
      setUserForm((prev) => ({ ...prev, password: "" }));
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  };

  const execute = async () => {
    try {
      const response = await clients.database.executeSql({ dsn, sql, maxRows: 200 });
      setColumns(response.columns);
      setRows(response.rows.map((row) => row.values));
      setMessage(t("database.queryResult", { rows: response.rows.length, affected: response.rowsAffected }));
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  };

  const loadTables = async () => {
    try {
      const response = await clients.database.listTables({ dsn });
      setTables(response.tables);
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  };

  const browse = async (table: string, offset: number) => {
    try {
      const response = await clients.database.browseTable({ dsn, table, limit: 50, offset });
      setBrowseName(table);
      setBrowseColumns(response.columns);
      setBrowseRows(response.rows.map((row) => row.values));
      setBrowseTotal(Number(response.totalRows));
      setBrowseOffset(offset);
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  };

  const importSqlFile = (file: File) => {
    const reader = new FileReader();
    reader.onload = () => {
      void (async () => {
        try {
          const response = await clients.database.importSql({
            dsn,
            sql: String(reader.result ?? "")
          });
          setMessage(t("database.sqlImported", { count: response.statementsExecuted }));
          setError("");
        } catch (err) {
          setError(safeError(err));
        }
      })();
    };
    reader.readAsText(file);
  };

  const loadOverview = async () => {
    try {
      const response = await clients.database.databaseOverview({ dsn });
      setOverview({
        version: response.version,
        connections: Number(response.activeConnections),
        uptime: Number(response.uptimeSeconds)
      });
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  };

  return (
    <section className="flex flex-col gap-5">
      <header className="flex flex-col gap-1">
        <h1 className="text-2xl font-semibold tracking-tight m-0">{t("database.title")}</h1>
        <p className="text-sm text-muted-foreground m-0">{t("database.subtitle")}</p>
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

      <Tabs defaultValue="sqlite">
        <TabsList>
          <TabsTrigger value="sqlite">SQLite</TabsTrigger>
          <TabsTrigger value="redis">Redis</TabsTrigger>
          <TabsTrigger value="dsn">{t("database.tabDsn")}</TabsTrigger>
        </TabsList>

        <TabsContent value="sqlite" className="mt-4">
          <Card>
            <CardHeader>
              <CardTitle>{t("database.sqliteCardTitle")}</CardTitle>
              <CardDescription>{t("database.sqliteCardDesc")}</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              <div className="grid gap-2">
                <UILabel htmlFor="sqlite-dirs">{t("database.sqliteDirsLabel")}</UILabel>
                <div className="flex gap-2">
                  <UIInput
                    id="sqlite-dirs"
                    placeholder={t("database.sqliteDirsPlaceholder")}
                    value={scanDirs}
                    onChange={(event) => setScanDirs(event.target.value)}
                  />
                  <UIButton variant="outline" onClick={() => void refreshSqlite()}>
                    <RefreshCw className="size-4" />
                    {t("database.scan")}
                  </UIButton>
                </div>
              </div>
              <div className="flex gap-2">
                <UIInput
                  className="flex-1"
                  placeholder={t("database.newSqlitePlaceholder")}
                  value={newSqlitePath}
                  onChange={(event) => setNewSqlitePath(event.target.value)}
                />
                <UIButton onClick={() => void createSqliteFile()}>
                  <Plus className="size-4" />
                  {t("database.create")}
                </UIButton>
              </div>
              {sqliteFiles.length === 0 ? (
                <div className="empty-state text-sm">{t("database.noSqliteFiles")}</div>
              ) : (
                <Table>
                  <TableHeader>
                    <UITableRow>
                      <TableHead>{t("database.colPath")}</TableHead>
                      <TableHead className="text-right">{t("database.colSize")}</TableHead>
                      <TableHead>{t("database.colModifiedAt")}</TableHead>
                      <TableHead className="text-right">{t("database.colActions")}</TableHead>
                    </UITableRow>
                  </TableHeader>
                  <TableBody>
                    {sqliteFiles.map((file) => (
                      <UITableRow key={file.path}>
                        <TableCell className="font-mono text-xs">{file.path}</TableCell>
                        <TableCell className="text-right tabular-nums">
                          {formatBytes(file.sizeBytes)}
                        </TableCell>
                        <TableCell className="text-xs text-muted-foreground">
                          {file.modifiedAtSeconds > 0n
                            ? formatDateTime(new Date(Number(file.modifiedAtSeconds) * 1000), locale)
                            : "-"}
                        </TableCell>
                        <TableCell className="text-right">
                          <UIButton
                            variant="outline"
                            size="sm"
                            onClick={() => void vacuumSqliteFile(file.path)}
                          >
                            <RotateCw className="size-3.5" />
                            VACUUM
                          </UIButton>
                        </TableCell>
                      </UITableRow>
                    ))}
                  </TableBody>
                </Table>
              )}
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="redis" className="mt-4">
          <Card>
            <CardHeader>
              <CardTitle>{t("database.redisCardTitle")}</CardTitle>
              <CardDescription>{t("database.redisCardDesc")}</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              <div className="grid gap-2">
                <UILabel htmlFor="redis-url">Redis URL</UILabel>
                <div className="flex gap-2">
                  <UIInput
                    id="redis-url"
                    placeholder={t("database.redisUrlPlaceholder")}
                    value={redisUrl}
                    onChange={(event) => setRedisUrl(event.target.value)}
                  />
                  <UIButton onClick={() => void refreshRedis()}>
                    <RefreshCw className="size-4" />
                    {t("database.connect")}
                  </UIButton>
                </div>
              </div>

              {redisInfo && (
                <RedisInfoView info={redisInfo} />
              )}
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="dsn" className="mt-4">
          <Card>
            <CardContent className="flex flex-col gap-3">
              <div className="grid gap-2">
                <UILabel htmlFor="db-dsn">{t("database.dsnLabel")}</UILabel>
                <div className="flex gap-2">
                  <UIInput
                    id="db-dsn"
                    placeholder={t("database.dsnPlaceholder")}
                    value={dsn}
                    onChange={(event) => setDsn(event.target.value)}
                  />
                  <UIButton onClick={() => void listDatabases()}>
                    <RefreshCw className="size-4" />
                    {t("database.connect")}
                  </UIButton>
                </div>
                <span className="text-xs text-muted-foreground">{t("database.dsnHint")}</span>
              </div>
            </CardContent>
          </Card>

          <Tabs defaultValue="databases" className="mt-4">
            <TabsList>
              <TabsTrigger value="databases">{t("database.databasesTab")}</TabsTrigger>
              <TabsTrigger value="users">{t("database.usersTab")}</TabsTrigger>
              <TabsTrigger value="sql">{t("database.sqlConsoleTab")}</TabsTrigger>
            </TabsList>

        <TabsContent value="databases" className="mt-4">
          <Card>
            <CardHeader>
              <CardTitle>{t("database.databaseListTitle")}</CardTitle>
              <CardDescription>{t("database.databaseListDesc")}</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-3">
              <div className="flex flex-wrap gap-2">
                <UIInput
                  className="flex-1 min-w-[200px]"
                  placeholder={t("database.newDbPlaceholder")}
                  value={newDbName}
                  onChange={(event) => setNewDbName(event.target.value)}
                />
                <UIButton onClick={() => void createDatabase()}>
                  <Plus className="size-4" />
                  {t("database.createDatabase")}
                </UIButton>
              </div>
              {databases.length === 0 ? (
                <div className="empty-state text-sm">{t("database.connectDsnFirst")}</div>
              ) : (
                <Table>
                  <TableHeader>
                    <UITableRow>
                      <TableHead>{t("database.colName")}</TableHead>
                      <TableHead className="text-right">{t("database.colActions")}</TableHead>
                    </UITableRow>
                  </TableHeader>
                  <TableBody>
                    {databases.map((database) => (
                      <UITableRow key={database}>
                        <TableCell className="font-medium">{database}</TableCell>
                        <TableCell className="text-right">
                          <UIButton
                            variant="outline"
                            size="sm"
                            onClick={() => void backupDatabase(database)}
                          >
                            <Download className="size-3.5" />
                            {t("database.backup")}
                          </UIButton>
                        </TableCell>
                      </UITableRow>
                    ))}
                  </TableBody>
                </Table>
              )}
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="users" className="mt-4">
          <Card>
            <CardHeader>
              <CardTitle>{t("database.createUserTitle")}</CardTitle>
              <CardDescription>{t("database.createUserDesc")}</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-3">
              <div className="grid gap-3 md:grid-cols-3">
                <div className="grid gap-2">
                  <UILabel htmlFor="db-user">{t("database.username")}</UILabel>
                  <UIInput
                    id="db-user"
                    value={userForm.username}
                    onChange={(event) => setUserForm((prev) => ({ ...prev, username: event.target.value }))}
                  />
                </div>
                <div className="grid gap-2">
                  <UILabel htmlFor="db-pass">{t("database.password")}</UILabel>
                  <UIInput
                    id="db-pass"
                    type="password"
                    value={userForm.password}
                    onChange={(event) => setUserForm((prev) => ({ ...prev, password: event.target.value }))}
                  />
                </div>
                <div className="grid gap-2">
                  <UILabel htmlFor="db-target">{t("database.grantDatabase")}</UILabel>
                  <UIInput
                    id="db-target"
                    value={userForm.database}
                    onChange={(event) => setUserForm((prev) => ({ ...prev, database: event.target.value }))}
                  />
                </div>
              </div>
              <div className="flex justify-end">
                <UIButton onClick={() => void createDatabaseUser()}>
                  <Plus className="size-4" />
                  {t("database.createAndGrant")}
                </UIButton>
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="sql" className="mt-4">
          <Card>
            <CardHeader>
              <CardTitle>{t("database.sqlConsoleTitle")}</CardTitle>
              <CardDescription>{t("database.sqlConsoleDesc")}</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-3">
              <div className="border border-border rounded-md overflow-hidden">
                <Editor
                  height="240px"
                  language="sql"
                  onChange={(value) => setSql(value ?? "")}
                  value={sql}
                  options={{ minimap: { enabled: false }, fontSize: 13 }}
                />
              </div>
              <div className="flex justify-between items-center gap-2 flex-wrap">
                <div className="flex items-center gap-2 flex-wrap">
                  <UIButton variant="outline" size="sm" onClick={() => void loadOverview()}>
                    {t("database.connectionOverview")}
                  </UIButton>
                  <label className="text-xs text-muted-foreground flex items-center gap-1">
                    {t("database.importSql")}
                    <input
                      type="file"
                      accept=".sql"
                      onChange={(event) => {
                        const file = event.target.files?.[0];
                        if (file) importSqlFile(file);
                        event.target.value = "";
                      }}
                    />
                  </label>
                </div>
                <div className="flex gap-2">
                  {columns.length > 0 && (
                    <UIButton
                      variant="outline"
                      onClick={() => downloadCsv(columns, rows, "query.csv")}
                    >
                      <Download className="size-4" />
                      {t("database.exportCsv")}
                    </UIButton>
                  )}
                  <UIButton onClick={() => void execute()}>
                    <Play className="size-4" />
                    {t("database.execute")}
                  </UIButton>
                </div>
              </div>
              {overview && (
                <div className="text-xs text-muted-foreground flex gap-4 flex-wrap">
                  <span>{t("database.overviewVersion", { value: overview.version || "-" })}</span>
                  <span>{t("database.overviewConnections", { value: overview.connections })}</span>
                  <span>
                    {t("database.overviewUptime", {
                      value: overview.uptime > 0 ? formatDuration(overview.uptime, t) : "-"
                    })}
                  </span>
                </div>
              )}
              {columns.length > 0 && (
                <div className="overflow-x-auto">
                  <table className="result-table">
                    <thead>
                      <tr>
                        {columns.map((column) => (
                          <th key={column}>{column}</th>
                        ))}
                      </tr>
                    </thead>
                    <tbody>
                      {rows.map((row, index) => (
                        <tr key={index}>
                          {row.map((value, cell) => (
                            <td key={cell}>{value}</td>
                          ))}
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </CardContent>
          </Card>

          <Card className="mt-4">
            <CardHeader>
              <CardTitle>{t("database.tableBrowseTitle")}</CardTitle>
              <CardDescription>{t("database.tableBrowseDesc")}</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-3">
              <div className="flex items-center gap-2 flex-wrap">
                <UIButton variant="outline" size="sm" onClick={() => void loadTables()}>
                  <RefreshCw className="size-3.5" />
                  {t("database.loadTables")}
                </UIButton>
                {tables.map((table) => (
                  <UIButton
                    key={table}
                    size="sm"
                    variant={browseName === table ? "default" : "outline"}
                    onClick={() => void browse(table, 0)}
                  >
                    {table}
                  </UIButton>
                ))}
              </div>
              {browseName && (
                <>
                  <div className="flex items-center justify-between text-sm text-muted-foreground flex-wrap gap-2">
                    <span>
                      {t("database.browseSummary", {
                        name: browseName,
                        total: browseTotal,
                        from: browseRows.length ? browseOffset + 1 : 0,
                        to: browseOffset + browseRows.length
                      })}
                    </span>
                    <div className="flex gap-2">
                      <UIButton
                        size="sm"
                        variant="outline"
                        onClick={() => downloadCsv(browseColumns, browseRows, `${browseName}.csv`)}
                      >
                        <Download className="size-3.5" />
                        {t("database.exportCsv")}
                      </UIButton>
                      <UIButton
                        size="sm"
                        variant="outline"
                        onClick={() => void browse(browseName, Math.max(0, browseOffset - 50))}
                      >
                        {t("database.prevPage")}
                      </UIButton>
                      <UIButton
                        size="sm"
                        variant="outline"
                        onClick={() => void browse(browseName, browseOffset + 50)}
                      >
                        {t("database.nextPage")}
                      </UIButton>
                    </div>
                  </div>
                  <div className="overflow-x-auto">
                    <table className="result-table">
                      <thead>
                        <tr>
                          {browseColumns.map((column) => (
                            <th key={column}>{column}</th>
                          ))}
                        </tr>
                      </thead>
                      <tbody>
                        {browseRows.map((row, index) => (
                          <tr key={index}>
                            {row.map((value, cell) => (
                              <td key={cell}>{value}</td>
                            ))}
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                </>
              )}
            </CardContent>
          </Card>
        </TabsContent>
          </Tabs>
        </TabsContent>
      </Tabs>
    </section>
  );
}

function RedisInfoView({ info }: { info: RedisInfo }) {
  const { t } = useLocale();
  if (!info.reachable) {
    return (
      <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
        {t("database.redisUnreachable", { error: info.error || t("database.unknownError") })}
      </div>
    );
  }
  const hits = Number(info.keyspaceHits);
  const misses = Number(info.keyspaceMisses);
  const hitRate = hits + misses > 0 ? (hits / (hits + misses)) * 100 : 0;
  const memoryPercent =
    info.maxMemoryBytes > 0n
      ? (Number(info.usedMemoryBytes) / Number(info.maxMemoryBytes)) * 100
      : 0;

  return (
    <div className="grid grid-cols-2 md:grid-cols-3 gap-3">
      <RedisStat label={t("database.statVersion")} value={info.version || "-"} />
      <RedisStat label={t("database.statMode")} value={info.mode || "-"} />
      <RedisStat label={t("database.statClients")} value={String(info.connectedClients)} />
      <RedisStat
        label={t("database.statUsedMemory")}
        value={formatBytes(info.usedMemoryBytes)}
        detail={
          info.maxMemoryBytes > 0n
            ? `${memoryPercent.toFixed(1)}% / ${formatBytes(info.maxMemoryBytes)}`
            : t("database.statNoMaxMemory")
        }
      />
      <RedisStat label={t("database.statEvictionPolicy")} value={info.maxMemoryPolicy || "noeviction"} />
      <RedisStat
        label={t("database.statHitRate")}
        value={`${hitRate.toFixed(1)}%`}
        detail={t("database.statHitRateDetail", { hits, misses })}
      />
      <RedisStat label={t("database.statTotalCommands")} value={String(info.totalCommandsProcessed)} />
      <RedisStat label={t("database.statUptime")} value={formatDuration(info.uptimeSeconds, t)} />
    </div>
  );
}

function RedisStat({
  label,
  value,
  detail
}: {
  label: string;
  value: string;
  detail?: string;
}) {
  return (
    <div className="flex flex-col gap-0.5 rounded-md border border-border bg-card p-3">
      <span className="text-xs text-muted-foreground">{label}</span>
      <span className="font-medium tabular-nums">{value}</span>
      {detail && <span className="text-xs text-muted-foreground">{detail}</span>}
    </div>
  );
}

// Phase E:常见 Cron 任务预设,适合微型 VPS 上的日常维护。
// label 用 MessageKey 而不是字面量,因为这是 module 级常量,渲染时才知道当前语言。
const CRON_PRESETS: Array<{ id: string; labelKey: MessageKey; cron: string; command: string }> = [
  {
    id: "sqlite-daily-backup",
    labelKey: "database.cronPresetSqliteDaily",
    cron: "0 0 3 * * *",
    command: "tar czf /var/backups/sqlite-$(date +%F).tgz /var/lib/rustpanel/sqlite/"
  },
  {
    id: "restic-weekly",
    labelKey: "database.cronPresetResticWeekly",
    cron: "0 0 4 * * 0",
    command: "restic -r $RESTIC_REPO backup /var/lib /etc --tag weekly"
  },
  {
    id: "logrotate-monthly",
    labelKey: "database.cronPresetLogrotateMonthly",
    cron: "0 0 5 1 * *",
    command: "find /var/log -name '*.log' -mtime +30 -delete"
  },
  {
    id: "disk-alert",
    labelKey: "database.cronPresetDiskAlert",
    cron: "0 */15 * * * *",
    command: "df / | awk 'NR==2 && $5+0>80 {print \"disk \"$5}' | logger -t rustpanel"
  },
  {
    id: "ssl-renew-check",
    labelKey: "database.cronPresetSslRenew",
    cron: "0 0 2 * * *",
    command: "rustpanel-backend --renew-certs"
  },
  {
    id: "fail2ban-status",
    labelKey: "database.cronPresetFail2ban",
    cron: "0 0 */6 * * *",
    command: "fail2ban-client status | logger -t rustpanel"
  }
];

export function CronPanel({ clients }: { clients: Clients }) {
  const { t } = useLocale();
  const [tasks, setTasks] = useState<CronTask[]>([]);
  const [form, setForm] = useState({ name: "daily-backup", cron: "0 0 2 * * *", command: "echo ok" });
  const [presetId, setPresetId] = useState("custom");
  const [log, setLog] = useState("");
  // 编辑中的任务;null 表示新建
  const [editing, setEditing] = useState<CronTask | null>(null);

  const load = async () => {
    const response = await clients.cron.listCronTasks({});
    setTasks(response.tasks);
  };

  useMountEffect(() => load());

  const saveTask = async () => {
    try {
      // CreateCronTask 按 id 覆盖,带上原 id 就是编辑
      await clients.cron.createCronTask({
        task: {
          id: editing?.id ?? "",
          name: form.name,
          cronExpression: form.cron,
          command: form.command,
          state: editing?.state ?? CronTaskState.ENABLED,
          timeoutSeconds: editing?.timeoutSeconds ?? 300n,
          nextRunAt: ""
        }
      });
      setLog(t("database.taskSaved", { name: form.name }));
      setEditing(null);
      await load();
    } catch (err) {
      setLog(safeError(err));
    }
  };

  const startEdit = (task: CronTask) => {
    setEditing(task);
    setPresetId("custom");
    setForm({ name: task.name, cron: task.cronExpression, command: task.command });
  };

  const cancelEdit = () => {
    setEditing(null);
    setForm({ name: "daily-backup", cron: "0 0 2 * * *", command: "echo ok" });
  };

  const toggleTask = async (task: CronTask) => {
    const next = task.state === CronTaskState.ENABLED ? CronTaskState.PAUSED : CronTaskState.ENABLED;
    try {
      await clients.cron.updateCronTaskState({ taskId: task.id, state: next });
      await load();
    } catch (err) {
      setLog(safeError(err));
    }
  };

  const deleteTask = async (task: CronTask) => {
    if (!window.confirm(t("database.confirmDeleteTask", { name: task.name }))) return;
    try {
      await clients.cron.deleteCronTask({ taskId: task.id });
      if (editing?.id === task.id) cancelEdit();
      setLog(t("database.taskDeleted", { name: task.name }));
      await load();
    } catch (err) {
      setLog(safeError(err));
    }
  };

  const runTask = async (task: CronTask) => {
    const run = await clients.cron.runCronTask({ taskId: task.id });
    setLog(
      t("database.taskRunResult", {
        name: task.name,
        state: CronRunState[run.run?.state ?? CronRunState.UNSPECIFIED]
      })
    );
    const logResponse = await clients.cron.getCronTaskLog({ taskId: task.id });
    setLog(logResponse.content);
  };

  return (
    <section className="page-grid">
      <header className="section-header full-span">
        <div>
          <h1>{t("database.cronTitle")}</h1>
          <p>{t("database.taskCount", { count: tasks.length })}</p>
        </div>
      </header>

      <div className="panel">
        <div className="panel-title">
          <Clock size={18} />
          <span>{editing ? t("database.editingTask", { name: editing.name }) : t("database.createTask")}</span>
        </div>
        <div className="input-row">
          <span>{t("database.commonTemplate")}</span>
          <Select
            value={presetId}
            onValueChange={(value) => {
              setPresetId(value);
              const preset = CRON_PRESETS.find((p) => p.id === value);
              if (preset) {
                setForm({ name: preset.id, cron: preset.cron, command: preset.command });
              }
            }}
          >
            <SelectTrigger>
              <SelectValue placeholder={t("database.custom")} />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="custom">{t("database.custom")}</SelectItem>
              {CRON_PRESETS.map((preset) => (
                <SelectItem key={preset.id} value={preset.id}>
                  {t(preset.labelKey)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <Input label={t("database.nameLabel")} value={form.name} onChange={(name) => setForm({ ...form, name })} />
        <Input label="Cron" value={form.cron} onChange={(cron) => setForm({ ...form, cron })} />
        <Input label={t("database.scriptLabel")} value={form.command} onChange={(command) => setForm({ ...form, command })} />
        <div className="flex gap-2">
          <button onClick={() => void saveTask()} type="button">
            <Save size={15} />
            {editing ? t("database.saveChanges") : t("database.save")}
          </button>
          {editing && (
            <button onClick={cancelEdit} type="button">{t("database.cancelEdit")}</button>
          )}
        </div>
      </div>

      <div className="panel wide-panel">
        <div className="panel-title"><Clock size={18} /><span>{t("database.taskList")}</span></div>
        {tasks.map((task) => (
          <div className="table-row" key={task.id}>
            <div>
              <strong>{task.name}</strong>
              <small>{task.cronExpression} · {task.command}</small>
            </div>
            <StatusPill
              label={task.state === CronTaskState.ENABLED ? t("database.enabled") : t("database.paused")}
              tone={task.state === CronTaskState.ENABLED ? "good" : "muted"}
            />
            <IconButton label={t("database.run")} icon={Play} onClick={() => void runTask(task)} />
            <IconButton
              label={task.state === CronTaskState.ENABLED ? t("database.paused") : t("database.enabled")}
              icon={task.state === CronTaskState.ENABLED ? Pause : RotateCw}
              onClick={() => void toggleTask(task)}
            />
            <IconButton label={t("database.edit")} icon={Pencil} onClick={() => startEdit(task)} />
            <IconButton label={t("database.delete")} icon={Trash2} onClick={() => void deleteTask(task)} />
          </div>
        ))}
      </div>

      <div className="panel log-panel">
        <div className="panel-title"><FileText size={18} /><span>{t("database.executionLog")}</span></div>
        <pre>{log}</pre>
      </div>
    </section>
  );
}
