import { app } from "./app";
import { apps } from "./apps";
import { backup } from "./backup";
import { common } from "./common";
import { components } from "./components";
import { dashboard } from "./dashboard";
import { database } from "./database";
import { files } from "./files";
import { network } from "./network";
import { ops } from "./ops";
import { security } from "./security";
import { settings } from "./settings";
import { sites } from "./sites";
import { terminal } from "./terminal";

// 迁移到哪个页面就在这里加一个命名空间;en/index.ts 用 `typeof zhCN` 做类型,
// 少一个命名空间或少一个 key 都是编译错误。
export const zhCN = {
  common,
  app,
  apps,
  backup,
  components,
  dashboard,
  database,
  files,
  network,
  ops,
  security,
  settings,
  sites,
  terminal
};

export type Messages = typeof zhCN;
