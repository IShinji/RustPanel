import { clients } from "./rpc";

/// Let's Encrypt 自 2024 起拒绝这三个 RFC 2606 保留域的 contact email。
/// 前后端都拦。
const FORBIDDEN_ACME_DOMAINS = [
  "@example.com",
  "@example.org",
  "@example.net"
];

export function isForbiddenAcmeEmailDomain(email: string): boolean {
  const lower = email.trim().toLowerCase();
  return FORBIDDEN_ACME_DOMAINS.some((d) => lower.endsWith(d));
}

/// 拿面板级 ACME 联系邮箱。优先后端持久化,没设过就 prompt 一次然后
/// **写回后端**(保存在 panel state,不存浏览器)。
/// 返回 null 表示用户取消;调用方应该中止当前操作。
export async function ensureAcmeEmail(): Promise<string | null> {
  try {
    const resp = await clients.ssl.getAcmeSettings({});
    const stored = resp.settings?.contactEmail?.trim() ?? "";
    if (stored && stored.includes("@") && !isForbiddenAcmeEmailDomain(stored)) {
      return stored;
    }
  } catch {
    // RPC 失败不致命,继续 prompt 流程,后续 update 失败时再提示
  }
  const input = window.prompt(
    "首次申请 SSL 证书需要一个真实邮箱(Let's Encrypt 用它做账户联系人,过期前发邮件提醒)。\n邮箱会存在面板里,所有浏览器共用,以后不再询问。",
    ""
  );
  if (!input) {
    return null;
  }
  const trimmed = input.trim();
  if (!trimmed.includes("@") || isForbiddenAcmeEmailDomain(trimmed)) {
    window.alert("请填一个真实邮箱;example.com / .org / .net 域会被 Let's Encrypt 拒掉。");
    return null;
  }
  try {
    // 先读一次保住 production 字段不被覆盖成默认 false。
    // (update 用整对象覆盖,proto 没有 PATCH 语义)
    let production = false;
    try {
      const current = await clients.ssl.getAcmeSettings({});
      production = current.settings?.production ?? false;
    } catch {
      // 读不到当默认 staging,后续用户在 Settings 页可以改
    }
    await clients.ssl.updateAcmeSettings({
      settings: { contactEmail: trimmed, production }
    });
  } catch (err) {
    console.warn("persist acme email failed:", err);
    window.alert("邮箱写入面板失败,这次申请会临时用这个邮箱,但下次还会再问一次。");
  }
  return trimmed;
}
