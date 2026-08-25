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
import { formatBytes, formatDuration, safeError } from "../lib/format";
import { type Clients } from "../lib/rpc";
import { Clock, Download, FileText, Play, Plus, RefreshCw, RotateCw, Save } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

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
      setError("请填写 SQLite 文件路径");
      return;
    }
    try {
      await clients.database.createSqliteFile({ path: newSqlitePath.trim() });
      setMessage(`已创建 ${newSqlitePath}`);
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
        `VACUUM 完成,${saved > 0 ? `节省 ${formatBytes(BigInt(saved))}` : "无空间可压缩"}`
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
      setMessage(`已连接,共 ${response.databases.length} 个数据库`);
      setError("");
    } catch (err) {
      setError(safeError(err));
    }
  }, [clients, dsn]);

  const createDatabase = async () => {
    if (!newDbName.trim()) return;
    try {
      await clients.database.createDatabase({ dsn, name: newDbName.trim() });
      setMessage(`数据库 ${newDbName} 已创建`);
      setNewDbName("");
      void listDatabases();
    } catch (err) {
      setError(safeError(err));
    }
  };

  const backupDatabase = async (database: string) => {
    try {
      const response = await clients.database.backupDatabase({ dsn, database });
      setMessage(`备份完成:${response.downloadUrl || "下载链接已生成"}`);
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
      setMessage(`用户 ${userForm.username} 已创建并授权 ${userForm.database}`);
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
      setMessage(`返回 ${response.rows.length} 行,影响 ${response.rowsAffected} 行`);
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
          setMessage(`已执行 ${response.statementsExecuted} 条语句`);
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
        <h1 className="text-2xl font-semibold tracking-tight m-0">数据库</h1>
        <p className="text-sm text-muted-foreground m-0">
          轻量优先:SQLite 单文件 → Redis(可选)→ 通用 DSN(MySQL / PostgreSQL,需 ≥ 256MB RAM)
        </p>
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
          <TabsTrigger value="dsn">通用 DSN</TabsTrigger>
        </TabsList>

        <TabsContent value="sqlite" className="mt-4">
          <Card>
            <CardHeader>
              <CardTitle>SQLite 文件</CardTitle>
              <CardDescription>
                嵌入式数据库,无需常驻进程,RustPanel 在低配 VPS 上的默认推荐。每个站点
                一个 .db 文件即可。
              </CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              <div className="grid gap-2">
                <UILabel htmlFor="sqlite-dirs">扫描目录(可选,空格或逗号分隔)</UILabel>
                <div className="flex gap-2">
                  <UIInput
                    id="sqlite-dirs"
                    placeholder="留空使用默认 /var/lib/rustpanel/sqlite, /srv/sqlite ..."
                    value={scanDirs}
                    onChange={(event) => setScanDirs(event.target.value)}
                  />
                  <UIButton variant="outline" onClick={() => void refreshSqlite()}>
                    <RefreshCw className="size-4" />
                    扫描
                  </UIButton>
                </div>
              </div>
              <div className="flex gap-2">
                <UIInput
                  className="flex-1"
                  placeholder="新建文件,如 /var/lib/rustpanel/sqlite/blog.db"
                  value={newSqlitePath}
                  onChange={(event) => setNewSqlitePath(event.target.value)}
                />
                <UIButton onClick={() => void createSqliteFile()}>
                  <Plus className="size-4" />
                  创建
                </UIButton>
              </div>
              {sqliteFiles.length === 0 ? (
                <div className="empty-state text-sm">
                  未发现 SQLite 文件 —— 检查扫描目录是否存在,或先创建一个
                </div>
              ) : (
                <Table>
                  <TableHeader>
                    <UITableRow>
                      <TableHead>路径</TableHead>
                      <TableHead className="text-right">大小</TableHead>
                      <TableHead>修改时间</TableHead>
                      <TableHead className="text-right">操作</TableHead>
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
                            ? new Date(Number(file.modifiedAtSeconds) * 1000).toLocaleString()
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
              <CardTitle>Redis 连接监控</CardTitle>
              <CardDescription>
                小内存机器推荐安装 redis-tuned(maxmemory 30MB + LRU)。这里读 INFO 命令展示
                关键指标。
              </CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              <div className="grid gap-2">
                <UILabel htmlFor="redis-url">Redis URL</UILabel>
                <div className="flex gap-2">
                  <UIInput
                    id="redis-url"
                    placeholder="redis://127.0.0.1:6379 或 rediss://user:pass@host:6380/0"
                    value={redisUrl}
                    onChange={(event) => setRedisUrl(event.target.value)}
                  />
                  <UIButton onClick={() => void refreshRedis()}>
                    <RefreshCw className="size-4" />
                    连接
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
                <UILabel htmlFor="db-dsn">数据库连接 DSN</UILabel>
                <div className="flex gap-2">
                  <UIInput
                    id="db-dsn"
                    placeholder="mysql://user:pass@host/db 或 postgres://user:pass@host/db"
                    value={dsn}
                    onChange={(event) => setDsn(event.target.value)}
                  />
                  <UIButton onClick={() => void listDatabases()}>
                    <RefreshCw className="size-4" />
                    连接
                  </UIButton>
                </div>
                <span className="text-xs text-muted-foreground">
                  提示:MySQL/PostgreSQL 容器运行时建议 ≥ 256MB RAM。低配机器请优先使用
                  SQLite Tab。
                </span>
              </div>
            </CardContent>
          </Card>

          <Tabs defaultValue="databases" className="mt-4">
            <TabsList>
              <TabsTrigger value="databases">数据库</TabsTrigger>
              <TabsTrigger value="users">用户</TabsTrigger>
              <TabsTrigger value="sql">SQL 控制台</TabsTrigger>
            </TabsList>

        <TabsContent value="databases" className="mt-4">
          <Card>
            <CardHeader>
              <CardTitle>数据库列表</CardTitle>
              <CardDescription>新建数据库,或对已有库一键备份</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-3">
              <div className="flex flex-wrap gap-2">
                <UIInput
                  className="flex-1 min-w-[200px]"
                  placeholder="新数据库名"
                  value={newDbName}
                  onChange={(event) => setNewDbName(event.target.value)}
                />
                <UIButton onClick={() => void createDatabase()}>
                  <Plus className="size-4" />
                  创建数据库
                </UIButton>
              </div>
              {databases.length === 0 ? (
                <div className="empty-state text-sm">先连接 DSN 以加载数据库列表</div>
              ) : (
                <Table>
                  <TableHeader>
                    <UITableRow>
                      <TableHead>名称</TableHead>
                      <TableHead className="text-right">操作</TableHead>
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
                            备份
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
              <CardTitle>创建数据库用户</CardTitle>
              <CardDescription>为指定数据库创建独立账号并授权</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-3">
              <div className="grid gap-3 md:grid-cols-3">
                <div className="grid gap-2">
                  <UILabel htmlFor="db-user">用户名</UILabel>
                  <UIInput
                    id="db-user"
                    value={userForm.username}
                    onChange={(event) => setUserForm((prev) => ({ ...prev, username: event.target.value }))}
                  />
                </div>
                <div className="grid gap-2">
                  <UILabel htmlFor="db-pass">密码</UILabel>
                  <UIInput
                    id="db-pass"
                    type="password"
                    value={userForm.password}
                    onChange={(event) => setUserForm((prev) => ({ ...prev, password: event.target.value }))}
                  />
                </div>
                <div className="grid gap-2">
                  <UILabel htmlFor="db-target">授权数据库</UILabel>
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
                  创建并授权
                </UIButton>
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="sql" className="mt-4">
          <Card>
            <CardHeader>
              <CardTitle>SQL 控制台</CardTitle>
              <CardDescription>仅返回最多 200 行结果</CardDescription>
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
                    连接概览
                  </UIButton>
                  <label className="text-xs text-muted-foreground flex items-center gap-1">
                    导入 .sql
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
                      导出 CSV
                    </UIButton>
                  )}
                  <UIButton onClick={() => void execute()}>
                    <Play className="size-4" />
                    执行
                  </UIButton>
                </div>
              </div>
              {overview && (
                <div className="text-xs text-muted-foreground flex gap-4 flex-wrap">
                  <span>版本: {overview.version || "-"}</span>
                  <span>活动连接: {overview.connections}</span>
                  <span>运行: {overview.uptime > 0 ? formatDuration(overview.uptime) : "-"}</span>
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
              <CardTitle>表浏览</CardTitle>
              <CardDescription>列出当前 DSN 下的表,分页查看数据(每页 50 行)</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-3">
              <div className="flex items-center gap-2 flex-wrap">
                <UIButton variant="outline" size="sm" onClick={() => void loadTables()}>
                  <RefreshCw className="size-3.5" />
                  加载表
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
                      {browseName} · 共 {browseTotal} 行 · 显示 {browseRows.length ? browseOffset + 1 : 0}-
                      {browseOffset + browseRows.length}
                    </span>
                    <div className="flex gap-2">
                      <UIButton
                        size="sm"
                        variant="outline"
                        onClick={() => downloadCsv(browseColumns, browseRows, `${browseName}.csv`)}
                      >
                        <Download className="size-3.5" />
                        导出 CSV
                      </UIButton>
                      <UIButton
                        size="sm"
                        variant="outline"
                        onClick={() => void browse(browseName, Math.max(0, browseOffset - 50))}
                      >
                        上一页
                      </UIButton>
                      <UIButton
                        size="sm"
                        variant="outline"
                        onClick={() => void browse(browseName, browseOffset + 50)}
                      >
                        下一页
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
  if (!info.reachable) {
    return (
      <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
        无法连接 Redis:{info.error || "未知错误"}
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
      <RedisStat label="版本" value={info.version || "-"} />
      <RedisStat label="模式" value={info.mode || "-"} />
      <RedisStat label="客户端" value={String(info.connectedClients)} />
      <RedisStat
        label="已用内存"
        value={formatBytes(info.usedMemoryBytes)}
        detail={
          info.maxMemoryBytes > 0n
            ? `${memoryPercent.toFixed(1)}% / ${formatBytes(info.maxMemoryBytes)}`
            : "无 maxmemory 限制"
        }
      />
      <RedisStat label="淘汰策略" value={info.maxMemoryPolicy || "noeviction"} />
      <RedisStat
        label="命中率"
        value={`${hitRate.toFixed(1)}%`}
        detail={`${hits} 命中 / ${misses} 未命中`}
      />
      <RedisStat label="累计命令" value={String(info.totalCommandsProcessed)} />
      <RedisStat label="运行时长" value={formatDuration(info.uptimeSeconds)} />
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
const CRON_PRESETS: Array<{ id: string; label: string; cron: string; command: string }> = [
  {
    id: "sqlite-daily-backup",
    label: "每日 SQLite 备份",
    cron: "0 0 3 * * *",
    command: "tar czf /var/backups/sqlite-$(date +%F).tgz /var/lib/rustpanel/sqlite/"
  },
  {
    id: "restic-weekly",
    label: "每周 restic 增量备份",
    cron: "0 0 4 * * 0",
    command: "restic -r $RESTIC_REPO backup /var/lib /etc --tag weekly"
  },
  {
    id: "logrotate-monthly",
    label: "每月清理日志",
    cron: "0 0 5 1 * *",
    command: "find /var/log -name '*.log' -mtime +30 -delete"
  },
  {
    id: "disk-alert",
    label: "磁盘 80% 告警",
    cron: "0 */15 * * * *",
    command: "df / | awk 'NR==2 && $5+0>80 {print \"disk \"$5}' | logger -t rustpanel"
  },
  {
    id: "ssl-renew-check",
    label: "SSL 续期检查(每天 02:00)",
    cron: "0 0 2 * * *",
    command: "rustpanel-backend ssl renew-due"
  },
  {
    id: "fail2ban-status",
    label: "fail2ban 状态汇报",
    cron: "0 0 */6 * * *",
    command: "fail2ban-client status | logger -t rustpanel"
  }
];

export function CronPanel({ clients }: { clients: Clients }) {
  const [tasks, setTasks] = useState<CronTask[]>([]);
  const [form, setForm] = useState({ name: "daily-backup", cron: "0 0 2 * * *", command: "echo ok" });
  const [presetId, setPresetId] = useState("custom");
  const [log, setLog] = useState("");

  const load = async () => {
    const response = await clients.cron.listCronTasks({});
    setTasks(response.tasks);
  };

  useEffect(() => {
    void load();
  }, []);

  const createTask = async () => {
    await clients.cron.createCronTask({
      task: {
        id: "",
        name: form.name,
        cronExpression: form.cron,
        command: form.command,
        state: CronTaskState.ENABLED,
        timeoutSeconds: 300n,
        nextRunAt: ""
      }
    });
    await load();
  };

  const runTask = async (task: CronTask) => {
    const run = await clients.cron.runCronTask({ taskId: task.id });
    setLog(`${task.name}: ${CronRunState[run.run?.state ?? CronRunState.UNSPECIFIED]}`);
    const logResponse = await clients.cron.getCronTaskLog({ taskId: task.id });
    setLog(logResponse.content);
  };

  return (
    <section className="page-grid">
      <header className="section-header full-span">
        <div>
          <h1>计划任务</h1>
          <p>{tasks.length} 个任务</p>
        </div>
      </header>

      <div className="panel">
        <div className="panel-title"><Clock size={18} /><span>创建任务</span></div>
        <div className="input-row">
          <span>常用模板</span>
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
              <SelectValue placeholder="自定义" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="custom">自定义</SelectItem>
              {CRON_PRESETS.map((preset) => (
                <SelectItem key={preset.id} value={preset.id}>
                  {preset.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <Input label="名称" value={form.name} onChange={(name) => setForm({ ...form, name })} />
        <Input label="Cron" value={form.cron} onChange={(cron) => setForm({ ...form, cron })} />
        <Input label="脚本" value={form.command} onChange={(command) => setForm({ ...form, command })} />
        <button onClick={() => void createTask()} type="button"><Save size={15} />保存</button>
      </div>

      <div className="panel wide-panel">
        <div className="panel-title"><Clock size={18} /><span>任务列表</span></div>
        {tasks.map((task) => (
          <div className="table-row" key={task.id}>
            <div>
              <strong>{task.name}</strong>
              <small>{task.cronExpression} · {task.command}</small>
            </div>
            <StatusPill label={task.state === CronTaskState.ENABLED ? "启用" : "暂停"} tone={task.state === CronTaskState.ENABLED ? "good" : "muted"} />
            <IconButton label="运行" icon={Play} onClick={() => void runTask(task)} />
          </div>
        ))}
      </div>

      <div className="panel log-panel">
        <div className="panel-title"><FileText size={18} /><span>执行日志</span></div>
        <pre>{log}</pre>
      </div>
    </section>
  );
}
