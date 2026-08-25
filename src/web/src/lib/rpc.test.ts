import { beforeEach, expect, test } from "bun:test";

// bun test 没有 DOM:rpc.ts 只用到 sessionStorage + window 事件,按需补最小垫片。
class MemoryStorage {
  private store = new Map<string, string>();
  getItem(key: string) {
    return this.store.get(key) ?? null;
  }
  setItem(key: string, value: string) {
    this.store.set(key, value);
  }
  removeItem(key: string) {
    this.store.delete(key);
  }
}

const listeners = new Map<string, Set<() => void>>();

Object.assign(globalThis, {
  sessionStorage: new MemoryStorage(),
  CustomEvent: class {
    type: string;
    constructor(type: string) {
      this.type = type;
    }
  },
  window: {
    location: { origin: "http://127.0.0.1:8080" },
    addEventListener(type: string, handler: () => void) {
      listeners.set(type, (listeners.get(type) ?? new Set()).add(handler));
    },
    removeEventListener(type: string, handler: () => void) {
      listeners.get(type)?.delete(handler);
    },
    dispatchEvent(event: { type: string }) {
      listeners.get(event.type)?.forEach((handler) => handler());
      return true;
    }
  }
});

const { appendAuthQuery, clearAuthToken, getAuthToken, onAuthChanged, setAuthToken } = await import(
  "./rpc"
);

beforeEach(() => {
  clearAuthToken();
});

test("token round-trips through session storage", () => {
  expect(getAuthToken()).toBeNull();
  setAuthToken("jwt-123");
  expect(getAuthToken()).toBe("jwt-123");
  clearAuthToken();
  expect(getAuthToken()).toBeNull();
});

test("appendAuthQuery leaves url untouched when logged out", () => {
  expect(appendAuthQuery("/api/fs/download?path=/etc")).toBe("/api/fs/download?path=/etc");
});

test("appendAuthQuery picks the right separator and escapes the token", () => {
  setAuthToken("a+b/c=");

  expect(appendAuthQuery("/api/terminal/ws")).toBe("/api/terminal/ws?token=a%2Bb%2Fc%3D");
  expect(appendAuthQuery("/api/fs/download?path=/etc")).toBe(
    "/api/fs/download?path=/etc&token=a%2Bb%2Fc%3D"
  );
});

test("auth change subscribers fire on login and logout, and unsubscribe works", () => {
  let hits = 0;
  const unsubscribe = onAuthChanged(() => {
    hits += 1;
  });

  setAuthToken("jwt-123");
  clearAuthToken();
  expect(hits).toBe(2);

  unsubscribe();
  setAuthToken("jwt-456");
  expect(hits).toBe(2);
});
