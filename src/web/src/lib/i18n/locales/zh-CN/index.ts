import { app } from "./app";
import { common } from "./common";
import { components } from "./components";
import { security } from "./security";

// 迁移到哪个页面就在这里加一个命名空间;en/index.ts 用 `typeof zhCN` 做类型,
// 少一个命名空间或少一个 key 都是编译错误。
export const zhCN = {
  common,
  app,
  components,
  security
};

export type Messages = typeof zhCN;
