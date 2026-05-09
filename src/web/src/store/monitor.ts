import { create } from "zustand";

import type { SystemStatus } from "../gen/rustpanel/v1/monitor_pb";

type MonitorState = {
  history: SystemStatus[];
  current?: SystemStatus;
  setCurrent: (status: SystemStatus) => void;
};

export const useMonitorStore = create<MonitorState>((set) => ({
  history: [],
  setCurrent: (status) =>
    set((state) => ({
      current: status,
      history: [...state.history.slice(-59), status]
    }))
}));
