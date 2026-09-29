import { CodeEditor as Editor } from "../components/code-editor";
import { IconButton } from "../components/form-controls";
import { ArchiveFormat, FileItem, FileKind, RecycleBinItem, SearchMatch } from "../gen/rustpanel/v1/fs_pb";
import { formatBytes } from "../lib/format";
import { languageForPath, parentPath } from "../lib/labels";
import { appendAuthQuery, authFetch, type Clients } from "../lib/rpc";
import { Archive, Download, FileText, Folder, FolderPlus, RefreshCw, RotateCw, Save, TerminalSquare, Trash2, Upload } from "lucide-react";
import { useRef, useState } from "react";
import { useMountEffect } from "../lib/hooks";
import { useLocale } from "../lib/i18n/locale-provider";

export function FileManager({ clients, openTerminal }: { clients: Clients; openTerminal: (cwd: string) => void }) {
  const { t } = useLocale();
  const [path, setPath] = useState("/");
  const [items, setItems] = useState<FileItem[]>([]);
  const [selected, setSelected] = useState<FileItem | undefined>();
  const [editorValue, setEditorValue] = useState("");
  const [menu, setMenu] = useState<{ x: number; y: number; item: FileItem } | undefined>();
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState<SearchMatch[]>([]);
  const [recycleItems, setRecycleItems] = useState<RecycleBinItem[]>([]);
  const inputRef = useRef<HTMLInputElement | null>(null);

  const load = async (nextPath = path) => {
    const response = await clients.files.listDirectory({ path: nextPath, recursive: false });
    const recycle = await clients.files.listRecycleBin({});
    setPath(nextPath);
    setItems(response.items);
    setRecycleItems(recycle.items);
  };

  useMountEffect(() => load("/"));

  const openItem = async (item: FileItem) => {
    setSelected(item);
    if (item.kind === FileKind.DIRECTORY) {
      await load(item.path);
      return;
    }
    const response = await clients.files.readFile({ path: item.path });
    setEditorValue(new TextDecoder().decode(response.content));
  };

  const saveFile = async () => {
    if (!selected) return;
    await clients.files.saveFile({ path: selected.path, content: new TextEncoder().encode(editorValue) });
    await load(path);
  };

  const upload = async (files: FileList | null) => {
    if (!files?.length) return;
    for (const file of files) {
      if (file.size > 5 * 1024 * 1024) {
        await uploadInChunks(file);
      } else {
        const form = new FormData();
        form.append("file", file);
        await authFetch(`/api/fs/upload?path=${encodeURIComponent(path)}`, {
          method: "POST",
          body: form
        });
      }
    }
    await load(path);
  };

  const uploadInChunks = async (file: File) => {
    const chunkSize = 1024 * 1024;
    const totalChunks = Math.ceil(file.size / chunkSize);
    const uploadId = `${Date.now()}-${file.name}`;
    for (let chunkIndex = 0; chunkIndex < totalChunks; chunkIndex += 1) {
      const chunk = file.slice(chunkIndex * chunkSize, Math.min(file.size, (chunkIndex + 1) * chunkSize));
      await authFetch(`/api/fs/upload/chunk?path=${encodeURIComponent(path)}&upload_id=${encodeURIComponent(uploadId)}&chunk_index=${chunkIndex}&total_chunks=${totalChunks}&file_name=${encodeURIComponent(file.name)}`, {
        method: "POST",
        body: chunk
      });
    }
  };

  const deleteItem = async (item: FileItem) => {
    await clients.files.deletePath({ path: item.path, recursive: item.kind === FileKind.DIRECTORY });
    setMenu(undefined);
    await load(path);
  };

  const search = async () => {
    const response = await clients.files.searchFiles({
      rootPath: path,
      query: searchQuery,
      regex: true,
      maxResults: 100
    });
    setSearchResults(response.matches);
  };

  const restoreRecycleItem = async (item: RecycleBinItem) => {
    await clients.files.restoreRecycleItem({ id: item.id });
    await load(path);
  };

  const archiveItem = async (item: FileItem) => {
    await clients.files.createArchive({
      sourcePaths: [item.path],
      archivePath: `${path.replace(/\/$/, "")}/${item.name}.zip`,
      format: ArchiveFormat.ZIP
    });
    setMenu(undefined);
    await load(path);
  };

  return (
    <section className="file-grid">
      <header className="section-header full-span">
        <div>
          <h1>{t("files.title")}</h1>
          <p>{path}</p>
        </div>
        <div className="toolbar">
          <IconButton label={t("files.refresh")} icon={RefreshCw} onClick={() => void load(path)} />
          <IconButton
            label={t("files.newFolder")}
            icon={FolderPlus}
            onClick={() => void clients.files.createDirectory({ path: `${path.replace(/\/$/, "")}/new-folder` }).then(() => load(path))}
          />
          <IconButton label={t("files.upload")} icon={Upload} onClick={() => inputRef.current?.click()} />
          <IconButton label={t("files.terminal")} icon={TerminalSquare} onClick={() => openTerminal(path)} />
          <input
            className="toolbar-input"
            onChange={(event) => setSearchQuery(event.target.value)}
            placeholder={t("files.searchPlaceholder")}
            value={searchQuery}
          />
          <IconButton label={t("files.search")} icon={FileText} onClick={() => void search()} />
          <input hidden multiple onChange={(event) => void upload(event.target.files)} ref={inputRef} type="file" />
        </div>
      </header>

      <div
        className="panel file-list"
        onDragOver={(event) => event.preventDefault()}
        onDrop={(event) => {
          event.preventDefault();
          void upload(event.dataTransfer.files);
        }}
      >
        <button className="breadcrumb" onClick={() => void load(parentPath(path))} type="button">
          ../
        </button>
        {items.map((item) => (
          <button
            className={selected?.path === item.path ? "file-row active" : "file-row"}
            key={item.path}
            onClick={() => void openItem(item)}
            onContextMenu={(event) => {
              event.preventDefault();
              setMenu({ x: event.clientX, y: event.clientY, item });
            }}
            type="button"
          >
            {item.kind === FileKind.DIRECTORY ? <Folder size={16} /> : <FileText size={16} />}
            <span>{item.name}</span>
            <small>{formatBytes(item.sizeBytes)}</small>
          </button>
        ))}
      </div>

      <div className="panel editor-panel">
        <div className="panel-title">
          <FileText size={18} />
          <span>{selected?.name ?? t("files.noFileSelected")}</span>
          {selected && selected.kind !== FileKind.DIRECTORY && (
            <>
              <IconButton label={t("files.save")} icon={Save} onClick={() => void saveFile()} />
              <IconButton
                label={t("files.download")}
                icon={Download}
                onClick={() => {
                  // 浏览器跳转无法带 Authorization header,所以把 token 拼到 query 里
                  window.location.href = appendAuthQuery(`/api/fs/download?path=${encodeURIComponent(selected.path)}`);
                }}
              />
            </>
          )}
        </div>
        <Editor
          height="520px"
          language={languageForPath(selected?.name ?? "")}
          onChange={(value) => setEditorValue(value ?? "")}
          options={{ minimap: { enabled: false }, fontSize: 13 }}
          value={editorValue}
        />
      </div>

      <div className="panel full-span">
        <div className="panel-title"><Trash2 size={18} /><span>{t("files.recycleBin")}</span></div>
        <div className="table-list">
          {recycleItems.slice(0, 8).map((item) => (
            <div className="table-row" key={item.id}>
              <div>
                <strong>{item.originalPath}</strong>
                <small>{item.recyclePath}</small>
              </div>
              <IconButton label={t("files.restore")} icon={RotateCw} onClick={() => void restoreRecycleItem(item)} />
            </div>
          ))}
          {!recycleItems.length && <div className="empty-state">{t("files.recycleBinEmpty")}</div>}
        </div>
      </div>

      <div className="panel full-span">
        <div className="panel-title"><FileText size={18} /><span>{t("files.fullTextSearch")}</span></div>
        <div className="table-list">
          {searchResults.map((match) => (
            <div className="table-row" key={`${match.path}-${match.lineNumber}`}>
              <div>
                <strong>{match.path}:{match.lineNumber}</strong>
                <small>{match.line}</small>
              </div>
            </div>
          ))}
          {!searchResults.length && <div className="empty-state">{t("files.noResults")}</div>}
        </div>
      </div>

      {menu && (
        <div className="context-menu" style={{ left: menu.x, top: menu.y }}>
          <button onClick={() => void archiveItem(menu.item)} type="button">
            <Archive size={15} />
            {t("files.archive")}
          </button>
          <button onClick={() => void deleteItem(menu.item)} type="button">
            <Trash2 size={15} />
            {t("files.delete")}
          </button>
        </div>
      )}
    </section>
  );
}

// Phase B 后续修补:把"软件商店"从 DockerApps 抽成独立页,不再跟着
// Docker daemon 一起挂掉。OpenVZ 上 docker.* 全部 reject 也能正常使用。
