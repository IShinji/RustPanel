import { expect, test } from "bun:test";

import {
  appStateVariant,
  auditLevelVariant,
  firewallActionLabel,
  firewallDirectionLabel,
  firewallProtocolLabel,
  languageForPath,
  parentPath,
  sshAlgorithmLabel,
  wafKindLabel
} from "./labels";
import {
  FirewallAction,
  FirewallDirection,
  FirewallProtocol,
  SshKeyAlgorithm,
  WafRuleKind
} from "../gen/rustpanel/v1/security_pb";

test("audit level maps to badge tone, unknown falls back to muted", () => {
  expect(auditLevelVariant("ERROR")).toBe("destructive");
  expect(auditLevelVariant("warn")).toBe("warning");
  expect(auditLevelVariant("notice")).toBe("info");
  expect(auditLevelVariant("whatever")).toBe("muted");
});

test("app state maps to badge tone by keyword", () => {
  expect(appStateVariant("running")).toBe("success");
  expect(appStateVariant("up")).toBe("success");
  expect(appStateVariant("exited with error")).toBe("destructive");
  expect(appStateVariant("restarting")).toBe("warning");
  expect(appStateVariant("")).toBe("muted");
});

test("security enums render Chinese labels with a dash fallback", () => {
  expect(firewallProtocolLabel(FirewallProtocol.TCP)).toBe("TCP");
  expect(firewallProtocolLabel(FirewallProtocol.UNSPECIFIED)).toBe("-");
  expect(firewallActionLabel(FirewallAction.DENY)).toBe("屏蔽");
  expect(firewallDirectionLabel(FirewallDirection.OUTBOUND)).toBe("出站");
  expect(wafKindLabel(WafRuleKind.SQL_INJECTION)).toBe("SQL 注入");
  expect(sshAlgorithmLabel(SshKeyAlgorithm.ED25519)).toBe("Ed25519");
});

test("editor language is derived from the extension", () => {
  expect(languageForPath("main.rs")).toBe("rust");
  expect(languageForPath("nginx.CONF")).toBe("plaintext");
  expect(languageForPath("docker-compose.yaml")).toBe("yaml");
  expect(languageForPath("noextension")).toBe("plaintext");
});

test("parentPath walks up one level and stops at root", () => {
  expect(parentPath("/")).toBe("/");
  expect(parentPath("/www/wwwroot/site")).toBe("/www/wwwroot");
  expect(parentPath("/www")).toBe("/");
});
